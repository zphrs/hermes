mod map;
pub use map::GenericNat;

pub use map::easy;
pub use map::hard;
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

#[expect(private_bounds)]
pub struct Nat<Mapping: Map> {
    internal: MachineRef<Network>,
    _marker: PhantomData<Mapping>,
    inner_machine: Rc<BasicMachine>,
}

pub type HardNat = Nat<hard::Symmetric>;
pub type EasyNat = Nat<easy::PortRestrictedCone>;

#[expect(private_bounds)]
impl<Mapping: Map + 'static> Nat<Mapping> {
    pub fn new(external_network: MachineRef<Network>) -> Self
    where
        Mapping: Default,
    {
        let inner_machine: Rc<_> = BasicMachine::new(tokio::sync::Semaphore::MAX_PERMITS).into();

        let out = Self {
            internal: Sim::add_machine(Default::default()),
            inner_machine,
            _marker: Default::default(),
        };
        let (our_v4, our_v6) = external_network.get().borrow_mut().add_machine(&out);
        info!("nat addrs: {}, {}", our_v4, our_v6);
        {
            let gotten = out.internal.get();
            let mut borrowed_internal = gotten.borrow_mut();
            borrowed_internal.add_machine_with_range(
                &*out.inner_machine,
                Ipv6Network::new(0u128.into(), 0).unwrap().into(),
            );
            borrowed_internal.add_machine_with_range(
                &*out.inner_machine,
                Ipv4Network::new(0u32.into(), 0).unwrap().into(),
            );
        }
        let internal = out.internal;
        let inner_machine = out.inner_machine.clone();
        out.inner_machine.spawn_local(async move {
            let mut mapping: Mapping = Default::default();
            while let Some(bytes) = inner_machine.read().await {
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
    pub fn lan(&self) -> MachineRef<Network> {
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
