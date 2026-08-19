use std::fmt::{Debug, Display};

use rpc::in_memory_transport::{self, ConnPair};

pub mod states {
    //! only allows toggling from state A to state B and back
    use rpc::{define_prioritized, method::not_applicable, state::priority};

    pub mod transition {
        use std::{convert::Infallible, marker::PhantomData};

        use rpc::{
            method::{can_transition, is_leaf},
            state,
        };

        /// transition method
        ///
        /// Unconditionally goes from current state to state To
        pub struct Method<To: rpc::State>(PhantomData<To>);

        impl<To: rpc::State> Method<To> {
            pub fn new() -> Self {
                Self(PhantomData)
            }
        }

        impl<To: rpc::State> Default for Method<To> {
            fn default() -> Self {
                Self::new()
            }
        }

        impl<To: rpc::State> rpc::Method for Method<To> {
            type Req = ();

            type Res = state::Wrapper<To>;

            type CanTransition = can_transition::True;

            type IsLeaf = is_leaf::True;
        }

        impl<To: rpc::State, RootMethod: rpc::method::Ancestor<Self>> rpc::Handler<RootMethod>
            for Method<To>
        {
            type Error = Infallible;

            async fn handle<Replier: rpc::ReplyHelper<RootMethod, Self>>(
                &mut self,
                replier: Replier,
                (): rpc::ReqOf<Self>,
            ) -> rpc::traits::HandlerResult<RootMethod, Self, Replier, Self::Error> {
                let res = replier.new_wrapper();
                replier.reply(res).await
            }
        }
    }

    pub struct A;

    impl rpc::State for A {
        type ClientHandles = not_applicable::NotApplicable;

        type ServerHandles = transition::Method<B>;
    }

    define_prioritized!(A, priority::server_wins);

    pub struct B;
    impl rpc::State for B {
        type ClientHandles = not_applicable::NotApplicable;

        type ServerHandles = transition::Method<A>;
    }
    define_prioritized!(B, priority::server_wins);

    pub type Entrypoint = A;
}

/// handles a transition from A to B and then from B to A, looping between
/// these two handlers
pub async fn server<Conn: rpc::transport::Connection + Clone>(conn: Conn) -> anyhow::Result<()>
where
    <Conn as rpc::transport::Client>::Error: Send + Sync + Debug + Display + 'static,
{
    let mut entrypoint_cursor =
        rpc::machine_cursor::MachineCursorServer::<states::Entrypoint, _>::new(conn);
    loop {
        let mut handler = states::transition::Method::new();
        let (processor, requester) = entrypoint_cursor.into_children_with_handler(&mut handler);

        let (res, transition) = processor
            .handle_transition_request()
            .await?
            .next_with_requester(requester)
            .await?
            .extract_res();

        let b_cursor = transition.finish(res);

        let mut handler = states::transition::Method::new();

        let (processor, requester) = b_cursor.into_children_with_handler(&mut handler);

        let (res, transition) = processor
            .handle_transition_request()
            .await?
            .next_with_requester(requester)
            .await?
            .extract_res();

        entrypoint_cursor = transition.finish(res);
    }
}

pub async fn client<Conn: rpc::transport::Connection + Clone>(conn: Conn) -> anyhow::Result<()>
where
    <Conn as rpc::transport::Caller>::Error:
        Send + Sync + Debug + Display + 'static + std::error::Error,
{
    let entrypoint_cursor =
        rpc::machine_cursor::MachineCursorClient::<states::Entrypoint, _>::new(conn);

    let b_cursor = from_a_to_b(entrypoint_cursor).await?;
    let a_cursor = from_b_to_a(b_cursor).await?;
    let b_cursor = from_a_to_b(a_cursor).await?;
    // we could keep going back and forth indefinitely
    // close cursor & connection by dropping
    drop(b_cursor);
    Ok(())
}

/// transition from state B to state A
async fn from_b_to_a<Conn: rpc::transport::Connection + Clone>(
    b_cursor: rpc::MachineCursor<states::B, Conn, rpc::state::role::Client>,
) -> anyhow::Result<rpc::MachineCursor<states::A, Conn, rpc::state::role::Client>>
where
    <Conn as rpc::transport::Caller>::Error:
        Send + Sync + Debug + Display + 'static + std::error::Error,
    <Conn as rpc::Caller>::Error: Debug + Display,
{
    let mut handler = rpc::method::not_applicable::Handler;
    let (processor, requester) = b_cursor.into_children_with_handler(&mut handler);
    let (res, transition) = requester
        .request_transition(())
        .next()
        .await?
        .next()
        .await?
        .assert_need_processor()
        .extract_res();
    let a_cursor = transition.finish(processor, res).await?;
    Ok(a_cursor)
}

/// transition from state A to state B
async fn from_a_to_b<Conn: rpc::transport::Connection + Clone>(
    entrypoint_cursor: rpc::MachineCursor<states::A, Conn, rpc::state::role::Client>,
) -> anyhow::Result<rpc::MachineCursor<states::B, Conn, rpc::state::role::Client>>
where
    <Conn as rpc::transport::Caller>::Error: Send + Sync + 'static + std::error::Error,
{
    let mut handler = rpc::method::not_applicable::Handler;
    let (processor, requester) = entrypoint_cursor.into_children_with_handler(&mut handler);
    let (res, transition) = requester
        .request_transition(())
        .next()
        .await?
        .next()
        .await?
        .assert_need_processor()
        .extract_res();
    let b_cursor = transition.finish(processor, res).await?;
    Ok(b_cursor)
}

#[tokio::main]
pub async fn main() {
    let network = in_memory_transport::Network::new();
    let ConnPair {
        client_conn,
        server_conn,
    } = in_memory_transport::setup_conn(0, 1, &network).await;

    let server_jh = tokio::spawn(server(server_conn));
    let client_jh = tokio::spawn(client(client_conn));

    client_jh.await.unwrap().unwrap();
    let _err = server_jh
        .await
        .unwrap()
        .expect_err("should exit with a connection aborted error");
}
