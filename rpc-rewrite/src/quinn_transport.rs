pub mod client;
pub mod end_to_end_socket;
pub mod server;
pub mod skip_server_verification;

pub mod dens_runtime {
    use quinn::TokioRuntime;

    #[derive(Debug)]
    pub struct DensRuntime;

    impl quinn::Runtime for DensRuntime {
        fn new_timer(&self, i: std::time::Instant) -> std::pin::Pin<Box<dyn quinn::AsyncTimer>> {
            TokioRuntime.new_timer(i)
        }

        fn spawn(&self, future: std::pin::Pin<Box<dyn Future<Output = ()> + Send>>) {
            TokioRuntime.spawn(future)
        }

        fn wrap_udp_socket(
            &self,
            _t: std::net::UdpSocket,
        ) -> std::io::Result<std::sync::Arc<dyn quinn::AsyncUdpSocket>> {
            unimplemented!()
        }
    }
}
