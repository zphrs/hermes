# Key Management

## Inspiration

Users will want to be able to define arbitrary ways of authenticating authorized
users (e.g. traditional oauth, SNARK). Also a given user might decide to change
how they do multi-device auth in a manner that is not backwards compatible with
their old way of doing auth.

Meanwhile other network nodes wants to be able to authenticate devices in
bounded constant time and space (e.g. standard p256 signatures) to avoid a DOS
attack where a given user gives a long history of previous transactions in order
to authenticate or uses a signature scheme that might take a long time to
process.

These needs can conflict. One common solution, OAuth2, is to simply have a
server somewhere issue JSON Web Tokens (Oauth2) for devices that then use those
tokens for future transactions. That server acts as the consensus mechanism for
determining who gets access to the system. This form of verification is very
cheap for the verifiers but can result in being locked out if the verifier
server goes down and can result in authorizing arbitrary users if the server
gets pwned. While corporations often can trust service-level agreements (SLAs)
to guarantee they will be financially compensated if they get locked out or
worse if the authorization server admits a user who shouldn't have been
admitted, consumers often can't trust such a guarantee, and can often end up 
locked out of OAuth accounts due to the whim of an algorithm that decided they
were abusing their Google/Facebook/Twitter account.

So, corporations would prefer to have OAuth support; meanwhile individuals with
a single device (or a password manager to sync accounts between services) would
prefer simple passkey support. Passkeys simply use a single private signature
key, synced between devices, in order to authenticate their accounts. Then it
relies on device management built into the OS in order to revoke access and
erase those private keys if one of the devices with that private key gets lost
or stolen. Another way to think about it is that it's OAuth colocated on the
user's device in an trusted execution environment with sysadmin access via
OS-level device management (Find My on Apple's OSes, Android Device Manager on
Android). This OAuth server can also be accessed by having a mobile phone with
the passkey private key in its enclave login to another computer by scanning a
QR Code to authorize a proxied websocket to the other computer. Then, the 
signature flow can be carried out over the websocket connection, similar to how
it works via inter-process (and inter-chip) communication locally.

That said, particularly paranoid people might not even trust Apple/Google to not
lock them out of their personal devices by issuing a remote wipe and also might
not trust Apple/Google to ensure their old device actually does get wiped (e.g.
blocking all network traffic from any known associated Apple/Google domain to
prevent the wipe signal from reaching the phone). In these cases, sharing one
private key across all devices can be worrisome. For these users, they might
prefer some form of threshold signing scheme like SNARK coupled with issuing
temporary tokens or something even more complex.


```
net_address = hash(hash(WASM contract) + public key)
```

WASM contract takes in: 
- public key (immutable per-network address)
- signature (generic byte array that your clients should know how to serialize
  into)
- action (specific enumerated action understood by either earth or sky nodes)

It returns a boolean representing whether the transaction is approved.



## Potential Shared Standard Library of Crypto Primitives

Contracts could have access to various crypto operations including P256 math
operations and hash functions like sha256.

### Advantages

- you can use simd instructions for identical results
- contract developers can just trust the P256 math happening under the hood
- contract sizes don't balloon too much 

### Disadvantages

- hard to add new algorithms without breaking forward compatibility between new
  contracts relying on the new algorithms
- the APIs available in the contract's source language might be easier to work
  with compared to WASM abi function calls.
- Performance might not be significantly better and might even be worse, even if
  the saving on contract size is real.



WASM contract can auto-fail if memory/time constraints run over and can be rate
limited per contract + IP with proof of work or simply timeout.


## Inspiration/Prior Work

- [Keyhive](https://www.inkandswitch.com/keyhive/notebook/)
- [Keybase](https://book.keybase.io/account)