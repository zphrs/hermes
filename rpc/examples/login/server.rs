use rpc::machine_cursor::MachineCursorServer;

use crate::states::{self, logged_in};

pub(crate) async fn server<R: rpc::transport::Connection + Clone>(conn: R) -> anyhow::Result<()>
where
    <R as rpc::transport::Client>::Error:
        Send + Sync + std::fmt::Debug + std::fmt::Display + 'static,
{
    let mut entrypoint_cursor = MachineCursorServer::<states::Entrypoint, _>::new(conn);
    loop {
        let logged_in_cursor = authenticate(entrypoint_cursor).await?;

        let mut handler = states::logged_in::ServerMethod;

        let (processor, requester) = logged_in_cursor.into_children_with_handler(&mut handler);

        let (res, processor_transition) = processor
            .handle_requests(logged_in::ping::Method)
            .await?
            .next_with_requester(requester)
            .await?
            .extract_res();

        match res {
            logged_in::RootRes::Logout(value) => {
                entrypoint_cursor = processor_transition.finish(value)
            }
            // is in the loopback tree so it wouldn't have broken
            // the handle_requests loop
            logged_in::RootRes::Ping(_) => unreachable!(),
        }
    }
}

pub(crate) async fn authenticate<R: rpc::transport::Connection + Clone>(
    mut entrypoint_cursor: MachineCursorServer<states::Entrypoint, R>,
) -> anyhow::Result<rpc::MachineCursor<states::logged_in::LoggedIn, R, rpc::state::role::Server>>
where
    <R as rpc::transport::Client>::Error:
        Send + Sync + std::fmt::Debug + std::fmt::Display + 'static,
{
    let logged_in_cursor = loop {
        let mut handler = states::entrypoint::login::Method;
        let (processor, requester) = entrypoint_cursor.into_children_with_handler(&mut handler);

        let (res, processor_transition) = processor
            .handle_transition_request()
            .await?
            .next_with_requester(requester)
            .await?
            .extract_res();

        match res.into_inner() {
            Ok(logged_in) => break processor_transition.finish(logged_in),
            Err(res) => entrypoint_cursor = processor_transition.finish(res.extract_wrapper()),
        }
    };
    Ok(logged_in_cursor)
}
