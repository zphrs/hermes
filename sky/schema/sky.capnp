@0xe34a4dab506e1fdc;

struct Address {
    bytes @0 :List(UInt8);
}

struct Ip {
    union {
        v4 @0 :UInt32;
        v6 @1 :List(UInt8);
    }
}

struct SocketAddr {
    ip @0 :Ip;
    port @1 :UInt16;
}

struct Node {
    addr @0 :SocketAddr;
}

interface Server {
    init @0 (intro :ServerIntro) -> (result :ServerReturn);
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
    address @0 :Address;
    invisible @1 :Bool;
}

struct ServerIntro {
    node @0 :Node;
    knownNodes @1 :List(Node);
}

struct ConnectRequest {
    to @0 :Address;
}

struct RoutingData {
    
}

struct ConnectRequestResponse {
    union {
        ok @0 :List(SocketAddr);
        redirect @1 :Node;
    }
}
