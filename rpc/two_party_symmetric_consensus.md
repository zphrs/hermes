# Two-Party Symmetric Consensus

Two-party symmetric consensus allows two parties to be confident making forward
progress, in an abstract sense, by enabling progress towards a common shared goal.

## Motivation

I 

The consensus problem in distributed systems is figuring out a way for all
parties to eventually "agree" on one value out of many "proposed" values.
Typically consensus is solved with algorithms like Raft and Paxos, but these
protocols are overkill for the two-party case and have no guarantee of halting in
an asynchronous setting.




This is because in the two-party case it is trivial to guarantee that there are
only ever two values proposed at a time and furthermore that once the other side
decides
