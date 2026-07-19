# State Transition Steps

## Requester

1. Send transition request
2. await response from transition request to turn into a receipt
3. With the handler/pendingTransitionRequest, if the receipt has the tiebreak
   flag set:
    1. if we have the handler then wait for the handler to turn into a
       pendingTransitionRequest
    2. continue with the receipt because the other side tiebroke in our
       direction. Maybe do a debug assert that we'd tiebreak in the same
       direction.
3. Else:
    1. continue with the receipt
2. with the receipt, send a notification that we've transitioned fully to the
   awaiting processor.

While waiting for the transition request to turn into a receipt, also wait
for the processor to receive a transition request. If the processor does receive a transition request then it will tiebreak. If our request wins then continue waiting for the transition request to turn into a receipt.

## Processor

1. Handle requests until a transition request arrives (resolves to IncomingTransitionRequest)
2. either provide a Requester or a RequestTransition future.
3. if provided a Requester:
    1. reply with the result and with the tiebreak flag unset and discard the requester. 
2. else if provided with a RequestTransition:
    1. tiebreak
    2. if tiebroken in the direction of the RequestTransition:
        1. break out of the processor tiebreaking (discard the IncomingTransitionRequest) and wait for the RequestTransition to finish
    4. else if tiebroken in the direction of the IncomingTransitionRequest:
        1. send off the IncomingTransitionRequest with the tiebreak flag set
4. wait for the notification that the requester has fully transitioned


## Datatypes needed for each step

- **concrete types**: connection, stream
- **generics/phantoms**: OldState, NewState, Role
