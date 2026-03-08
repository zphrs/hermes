use maxlen::MaxLen;
use minicbor::{CborLen, Decode, Encode};

// Example: Using derive for a simple struct
#[derive(Encode, Decode, CborLen, MaxLen)]
struct User {
    #[n(0)]
    id: u64,
    #[n(1)]
    active: bool,
}

// Example: Using derive for an enum (like EarthToSkyRequestValue)
#[derive(Encode, Decode, CborLen, MaxLen)]
#[cbor(flat)]
enum RequestType {
    #[n(0)]
    Ping,
    #[n(1)]
    GetUser(#[n(0)] u64),
    #[n(2)]
    UpdateUser(#[n(0)] User),
}

// Example: Using derive for nested structures
#[derive(Encode, Decode, CborLen, MaxLen)]
struct Request {
    #[n(0)]
    request_type: RequestType,
    #[n(1)]
    from: User,
}

// Example: Using derive with arrays (like the Register response)
// Note: Option and array impls are provided by the maxlen crate
#[derive(Encode, Decode, CborLen, MaxLen)]
struct Response {
    #[n(0)]
    users: Option<[Option<User>; 20]>,
}

fn main() {
    // Create the biggest possible instances
    let user = User::biggest_instantiation();
    println!("Biggest User: id={}, active={}", user.id, user.active);
    println!("User max CBOR length: {}", User::max_len());

    let _request_type = RequestType::biggest_instantiation();
    println!(
        "\nBiggest RequestType max CBOR length: {}",
        RequestType::max_len()
    );

    let _request = Request::biggest_instantiation();
    println!("Request max CBOR length: {}", Request::max_len());

    let _response = Response::biggest_instantiation();
    println!("Response max CBOR length: {}", Response::max_len());

    // Demonstrate that max_len() is cached
    println!("\nCalling max_len() again (should use cached value):");
    println!("User max CBOR length: {}", User::max_len());

    // Show how the enum picks the largest variant
    let variants = [
        RequestType::Ping,
        RequestType::GetUser(MaxLen::biggest_instantiation()),
        RequestType::UpdateUser(MaxLen::biggest_instantiation()),
    ];

    println!("\nEnum variant sizes:");
    for (i, variant) in variants.iter().enumerate() {
        println!("Variant {}: {} bytes", i, minicbor::len(variant));
    }

    let biggest = RequestType::biggest_instantiation();
    println!("Chosen biggest variant: {} bytes", minicbor::len(&biggest));
}
