use shared_schema::EarthNode;

pub trait Sky {
    fn nearby_earth_nodes() -> impl std::future::Future<Output = EarthNode>;
}
