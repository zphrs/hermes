# rpc-rewrite API guide for the room port

Paths relative to rpc-rewrite/src/. Compiled from a read-only survey of the working tree (not run). Templates to copy from: `cursor/tests/race/hybrid.rs` (both sides CanTransition, complete), `cursor/tests/hybrid.rs` (+states/entrypoint.rs), `cursor/tests/a_to_b.rs`, `cursor/tests/race.rs`, `cursor/tests/loopback.rs`.

## Model
- `Cursor<State, Role, C>`; `into_processor_and_requester(handler)` -> (Processor, Requester); needs `C: Clone`.
- Next state only via `CursorCredit` (from `request_transition` / `next::*`) + `Cursor::from_cursor_credit(credit, wrapper)`. `Cursor::new` only for `state::Entrypoint`.
- Server processor handles `S::ServerHandles`, server requester sends `S::ClientHandles`; client is the mirror.
- Processor method by handled branch type:
  - `Branch<Loopback>`: `loopback::BranchHandler`; `handle_loopback_requests()`.
  - `Branch<Transition>`: `TransitionBranchHandler` (`NextHandler`); `handle_transition_request(&mut buf, requester_of_NotApplicable)` or `handle_concurrent_transition_request(&mut buf)`.
  - `Branch<CanTransition>`: `can_transition::BranchHandler` (= `HybridBranchHandler`) + `Clone`; `handle_hybrid_concurrent_transition_requests(&'buf mut Vec<u8>)`.
- `RootHandler`/`RootMethod<M, BT>` wrap a single leaf only (Loopback/Transition), not CanTransition. For CanTransition write your own root enum + `can_transition::BranchHandler` (see `Root` in tests/race/hybrid.rs:57-164).
- Hybrid handler takes `self` by value and is CLONED per accepted stream => shared state in `Arc`/channels. `NextHandler` is one type for all transitions (make it an enum if needed). Return `None` for loopback leaves, `Some(next)` for transition leaves:
  ```rust
  RootRequest::Ping(r) => (replier.reply_with_leaf(r, &mut Ping).await?, None),
  RootRequest::Race(r) => { let (rc, next) = replier.transition_with_leaf(r, Race).await?; (rc, Some(next)) }
  ```
- Leaf traits: `LeafHandler::handle(&mut self, req) -> Res`; `TransitionLeafHandler::handle_transition(self, req, WrapperCredit<M>) -> (Res, NextHandler)`; `wrapper_credit.into()` gives `Wrapper<S>` if `ResOf<M>: Has<S>`. Each leaf `impl Descendant<Root>`; root req/res enums derive minicbor Encode/Decode/CborLen.

## Both sides may transition (hybrid) -- pattern from tests/race/hybrid.rs:186-256
```rust
let mut buf = Vec::new();
let mut processor_fut = pin!(processor.handle_hybrid_concurrent_transition_requests(&mut buf));
let mut read_into = Vec::new();
let need_requester = tokio::sync::Notify::new();
let to_requester = oneshot::channel();
let requester_fut = async {
    // loopback requests here (requester.request_loopback::<M>(req, &mut buf))
    match select(pin!(need_requester.notified()), pin!(<wait for trigger>)).await {
        Either::Left(_) => { to_requester.0.send(requester).ok().unwrap(); pending().await }  // hand requester back
        Either::Right(_) => Ok::<_,E>(requester.request_concurrent_transition::<Leaf>(req, &mut read_into).await?),
    }
};
let mut requester_fut = pin!(requester_fut);
let (won, credit) = match tiebreak(&mut processor_fut, &mut requester_fut).await? {
    tiebreak::With::Processor(pt) => { need_requester.notify_one();
        pt.next(async move { Ok(match select(requester_fut, to_requester.1).await {
            Either::Left((rt,_)) => RequesterOrRequesterTransition::RequesterTransition(rt?),
            Either::Right((r,_)) => RequesterOrRequesterTransition::Requester(r?) }) }).await? }
    tiebreak::With::Requester(rt) => rt.next(processor_fut).await?,
};
// won: Won::Processor{res: <root response enum>, next_handler} | Won::Requester{res: Wrapper<Next>}
Cursor::from_cursor_credit(credit, wrapper)
```
- `requester.request_concurrent_transition` consumes the requester, returns once the request is WRITTEN.
- Winner is dynamic: whoever commits first (reply-not-yet-arrived check in `cursor/transition/next/commit_or_defer.rs`); true tie => SERVER request wins. No Prioritized/server_wins exists. Design so either outcome is valid.
- Non-racing: client-only-requests => `requester.request_transition::<M>(req, &mut buf, processor)` (processor must be NotApplicable-handling) `-> (res, credit)`; server `processor.handle_transition_request(&mut buf, requester) -> (res, next_handler, credit)`. These are balanced; use for Entrypoint (client NotApplicable, server Transition). Do NOT use hybrid processor at Entrypoint (unbalanced ProcessorSacrificed uni-stream, see below).
- Close: `Cursor<S,Client>::close()` / `Cursor<S,Server>::wait_to_close()` only when both Handles = NotApplicable. Client calls close, server waits.

## Loopback / server-initiated
- `requester.request_loopback::<M>(req, &mut buf)` takes `&self`; concurrent OK; response may borrow `buf`.
- Server -> client: use the server's Requester (root = `ClientHandles`). No user-facing notification API: `io::notify` is private. Model Notify as `LeafLoopback` with `Res = ()` and await the ack. Server-initiated close: `requester.request_concurrent_transition::<CloseLeaf>` raced via `tiebreak`.

## Pitfalls
1. Keep `ProcessorFut` polled while our request is in flight.
2. Never drop a held-reply processor transition without reply/reject (peer sees `read::Error::Empty`).
3. Dropping an in-flight `request_loopback` future makes the NEXT `request_concurrent_transition` fail with `AbandonedLoopback`. Finish loopbacks; poll `need_requester` only between them.
4. Requester hand-back protocol (Notify + oneshot) as in the pattern above.
5. Transition leaf handlers must be cancel-safe & side-effect free (leaf may be dropped or its reply discarded). Do room mutations (add/remove user, notify others) AFTER `Won::*`, pass data via `NextHandler`.
6. In-flight loopbacks are DROPPED when a transition wins (no drain).
7. Hybrid processor with `Handler: Clone`; don't touch `buf` while `Won::Processor{res}` borrows it.
8. `RequesterOrRequesterTransition::Requester(r)` has `M = NotApplicable` => `Won::Requester{res}` is matched with `match res {}`.
