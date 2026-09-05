New way to do multi-device auth (and auth of a group in general). Ultimately
users will want to be able to define arbitrary ways of authenticating authorized
users and will want arbitrary ways to allow sending messages in between one
another within a group. Also a given user might decide to change how they do
multi-device auth in a manner that is not backwards compatible with their old
way of doing auth.

Meanwhile other network nodes wants to be able to authenticate users in bounded
constant time and space (e.g. standard p256 signatures) to avoid a DOS attack
where a given user gives a long history of previous transactions in order to
authenticate or uses a signature scheme that might take a long time to process.

Ultimately my solution is to use WASM contracts (bounded in space and time at
runtime) that return either yes, the transaction is authenticated, or no, the
transaction is not authenticated, based on the immutable public key, the hash of
the operation, and the mutable signature. The hash of the wasm contract will be
concatenated with the public key and then hashed to get the final address in the
network. While the public key could be hard-coded into the contract, that would
potentially result in storing needless duplicate wasm contracts that only differ
by public key. Deduplicating such common contracts is useful because replicators
wouldn't need to store a contract per user and could amortize the cost of
storing the contract. 

In the announcement of presence, the contract's contents is announced, alongside
the public key and some additional metadata not relevant here. Then nodes nearby
will store the wasm contract locally, deduplicating contracts if possible, and
forward the contract to any new nodes that come online and express an interest
in helping that user cache messages. 

If a user's contract uses an unbounded signature size then they will eventually
have to migrate to a new public key in order to reset the log. To do so, the old
contract code will sign one last redirect transaction that then should be
republished by the user regularly, alongside a standard announcement of presence
as described above. Ultimately it is on the user to have sufficient reputation
in this part of the address space (e.g. by doing proof of work) for their
redirect to stay online. Anyone who has possession of a valid signed "redirect"
is then allowed to sign operations that increase their reputation and make 
special AOPs that explicitly tell neighbors to drop any messages addressed to 
them and tell anyone querying about that exact address about the new location
the redirect announces.

Furthermore, a redirect AOP will override any other AOP for that address for as
long as the AOP is live (including other redirect AOPs), marking the old address
as essentially unusable for anything else. Note that hosting the AOP in the
neighborhood is not free and so redirected accounts should avoid announcing and
maintaining several redirect AOPs for longer than is necessary long because that
will eat into the ability for that node to do other useful work and work towards
increasing their storage capacity in their new neighborhood and will result in
the client participating in two neighborhoods at once. A redirect AOP should
only be maintained for long enough to hear from all of your friends that they
have all switched to your new address. Good clients will prioritize storing the
first redirect AOP they received and cache a hash of the first AOP they received
for an order of magnitude longer than they cache the AOP to ensure a malicious
party who has compromised the old authentication mechanism cannot overwrite
the first known redirect AOP. This does cut both ways so be sure to mark the
old account as compromised as soon as possible if a key has been compromised
in order to ensure your friends are redirected to your new location instead
of an adversary-controlled new address.

Once the redirect is signed, the old scheme's private keys should be discarded
and only the final signature should be kept. While it's impossible to prove
that such private keys are discarded, if they do leak then the most they can
do is try to suppress the redirect AOP. Once the redirect "gets out" in a
neighborhood, all future uses of that address are disabled until that redirect
AOP expires. Furthermore any benevolent clients who tried to send a message to 
that part of the address space would have received the redirect notice and thus
not bother even checking that old address again.

Ultimately this could open up an attack vector where an account gets compromised
and the attacker redirects the account to a new part of the address space. While
this is a possibility, allowing redirects is necessary to revoke compromised
keys without forcing verifiers to verify a possibly unbounded state. Ultimately
if your current key is compromised by an attacker before you are able to sign 
and announce a change of address then that account can be compromised for good.

Client applications that receive a redirect notice should notify your friends
that your address has changed and encourage confirmation outside of the 
application flow of your new address. 

Fortunately accounts are monetarily free and while it might take a while to get
in contact with your contacts and inform them of the new account, nothing stops
you from starting over with a new account. Also using password managers and
passkeys can almost fully eliminate the human risk of accidentally leaking the
secret key that replaces the typical password. Worst case you accidentally grant
a session token to an attacker that only allows temporary access to performing
actions like message deletion and announcements of presence. Ultimately your
address won't be lost except to an especially determined attacker.

That said, using an insecure client application could always make you vulnerable
so I strongly recommend building from source and auditing your account's WASM
auth contract source code and the code that deals with signing transactions.


