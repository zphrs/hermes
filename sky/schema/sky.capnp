@0xe34a4dab506e1fdc;

struct Id {
    # 256 bits, big endian
    key0 @0 :UInt64;
    key1 @1 :UInt64;
    key2 @2 :UInt64;
    key3 @3 :UInt64;
}

# 16 bytes
struct Ipv6 {
    left @0 :UInt64;
    right @1 :UInt64;
}
# 8 byte ptr | <Ipv6> or 4 byte ipv4 value
struct Ip {
    union {
        v4 @0 :UInt32;
        v6 @1 :Ipv6;
    }
}
# 8 byte ptr | 2 byte port value | 6 byte padding | <Ip>
struct SocketAddr {
    ip @0 :Ip;
    port @1 :UInt16;
}
# 8 byte ptr | <SocketAddr>
struct Node {
    addr @0 :SocketAddr;
}

struct ClientReturn {
    union {
        ok @0 :List(Node);
        redirect @1 :Node;
    }
}

struct ServerReturn {
    node @0 :Node;
    newNodes @1 :List(Node);
}


struct ClientIntro {
    address @0 :Id;
    invisible @1 :Bool;
}

struct FindNodeRequest {
    targetAddr @0: Id;
}

struct PeerRequest {
    union {
        ping @0: Void;
        findNode @1:FindNodeRequest;
    }
}

struct PingResponse {}

struct FindNodeResponse {
    nodes @0 :List(Node);
}

struct ConnectRequest {
    to @0 :Id;
}

struct RoutingData {
    
}

struct ConnectRequestResponse {
    union {
        ok @0 :List(SocketAddr);
        redirect @1 :Node;
    }
}
