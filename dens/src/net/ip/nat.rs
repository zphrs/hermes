mod map;
pub use map::GenericNat;

pub use map::easy;
pub use map::hard;
use std::time::Duration;
use std::{marker::PhantomData, net::IpAddr, rc::Rc};

use tracing::info;

use crate::{
    BasicMachine, Machine, Sim,
    net::{ip::Network, udp},
    sim::{MachineRef, machine::HasNic},
};
use bytes::BytesMut;
use ip_network::{Ipv4Network, Ipv6Network};
use map::Map;
use tracing::warn;

/// Network address translator.
///
/// Exposes its internal network via [`Nat::lan()`].
///
/// # Examples
///
///
#[expect(private_bounds)]
pub struct Nat<Mapping: Map> {
    internal: MachineRef<Network>,
    _marker: PhantomData<Mapping>,
    inner_machine: Rc<BasicMachine>,
}
// Endpoint ip and port dependent firewall, endpoint ip and port dependent nat.
pub type HardNat = Nat<hard::Symmetric>;
// Endpoint ip and port dependent firewall, EIN NAT
pub type EasyNat = Nat<easy::PortRestrictedCone>;

#[expect(private_bounds)]
impl<Mapping: Map + 'static> Nat<Mapping> {
    pub fn new_with_lan_instantiator(
        lan_instantiator: impl FnOnce() -> Network,
        external_network: MachineRef<Network>,
    ) -> Self
    where
        Mapping: Default,
    {
        let inner_machine: Rc<_> = BasicMachine::new(tokio::sync::Semaphore::MAX_PERMITS).into();

        let out = Self {
            // Uses a class c network because there's only 65,535 ports available
            // anyway so there's no point using a bigger network.
            //
            // This also saves the a and b private blocks for internets which
            // include a nat.
            internal: Sim::add_machine(lan_instantiator()),
            inner_machine,
            _marker: PhantomData,
        };
        let (our_v4, our_v6) = external_network.get().borrow_mut().add_machine(&out);
        info!("nat addrs: {}, {}", our_v4, our_v6);
        {
            let gotten = out.internal.get();
            let mut borrowed_internal = gotten.borrow_mut();
            borrowed_internal.add_machine_with_range(
                &*out.inner_machine,
                #[expect(clippy::missing_panics_doc, reason = "infallible")]
                Ipv6Network::new(0u128.into(), 0).unwrap().into(),
            );
            borrowed_internal.add_machine_with_range(
                &*out.inner_machine,
                #[expect(clippy::missing_panics_doc, reason = "infallible")]
                Ipv4Network::new(0u32.into(), 0).unwrap().into(),
            );
        }
        let internal = out.internal;
        let inner_machine = out.inner_machine.clone();
        out.inner_machine.spawn_local(async move {
            let mut mapping: Mapping = Default::default();
            loop {
                let bytes = match inner_machine.try_read() {
                    Ok(bytes) => bytes,
                    Err(e) => match e {
                        tokio::sync::mpsc::error::TryRecvError::Empty => {
                            tokio::time::sleep(Duration::from_millis(1)).await;
                            continue;
                        }
                        tokio::sync::mpsc::error::TryRecvError::Disconnected => break,
                    },
                };

                let Ok(mut packet) = udp::Packet::try_from_bytes(bytes) else {
                    warn!("malformed udp packet");
                    continue;
                };
                let our_addr: IpAddr = if packet.header().ip_header().is_v4() {
                    our_v4.into()
                } else {
                    our_v6.into()
                };
                let external_request = our_addr == packet.header().get_dst_addr().ip();
                if external_request {
                    // then route it to our internal network
                    let Some(internal_addr) = mapping.external_port_to_internal_addr(
                        packet.header().get_dst_addr().port(),
                        packet.header().get_src_addr(),
                    ) else {
                        warn!("discarding external request possibly due to firewall");
                        continue;
                    };
                    packet.set_dst_addr(internal_addr);
                    let mut serialized_packet = BytesMut::new();
                    packet.write_into_buf(&mut serialized_packet);
                    let Ok(()) = internal
                        .get()
                        .borrow_mut()
                        .try_send_packet(serialized_packet.freeze())
                    else {
                        warn!(
                            "failed to forward external packet to internal network, dropping packet"
                        );
                        continue;
                    };
                    continue;
                }
                let port = mapping.internal_addr_to_external_port(
                    packet.header().get_src_addr(),
                    packet.header().get_dst_addr(),
                );
                packet.set_src_addr((our_addr, port).into());
                let mut serialized_packet = BytesMut::new();
                packet.write_into_buf(&mut serialized_packet);
                let Ok(()) = external_network
                    .get()
                    .borrow()
                    .try_send_packet(serialized_packet.freeze())
                else {
                    warn!("failed to forward along, dropping packet");
                    continue;
                };
            }
            Ok(())
        });
        out
    }
    /// Creates a nat using the ip address block designated for carrier-grade
    /// NAT in [RFC 6598](https://www.rfc-editor.org/rfc/rfc6598#section-7).
    ///
    /// Specifically this is for second-layer NATs which MAY have NATs within
    /// them. In the real world, carrier-grade NATs comprise the second layer
    /// while residential and corporate NATs comprise the first.
    ///
    /// See [`Nat::new()`] to construct a standard NAT which uses the address
    /// range typically used for residential NATs (the kind created by
    /// routers).
    #[must_use]
    pub fn new_carrier_grade(external_network: MachineRef<Network>) -> Self
    where
        Mapping: Default,
    {
        Self::new_with_lan_instantiator(
            || {
                Network::new_with_ipv4_prefix(
                    #[expect(clippy::missing_panics_doc, reason = "infallible")]
                    crate::net::ip::Ipv4Prefix::new([100, 64, 0, 0].into(), 10).unwrap(),
                )
            },
            external_network,
        )
    }

    /// Creates a nat, using a [private class c
    /// Network](Network::new_private_class_c) for its internal
    /// [`lan`](Self::lan).
    pub fn new(external_network: MachineRef<Network>) -> Self
    where
        Mapping: Default,
    {
        Self::new_with_lan_instantiator(Network::new_private_class_c, external_network)
    }
    #[must_use]
    pub const fn lan(&self) -> MachineRef<Network> {
        self.internal
    }
}

impl<Mapping: Map + 'static> Machine for Nat<Mapping> {
    fn basic_machine(&self) -> std::rc::Rc<crate::BasicMachine> {
        self.inner_machine.clone()
    }

    fn is_idle(&self) -> bool {
        false
    }
}

impl<Mapping: Map + 'static> HasNic for Nat<Mapping> {
    fn nic(&self) -> crate::sim::machine::MachineNic {
        self.inner_machine.nic()
    }
}
