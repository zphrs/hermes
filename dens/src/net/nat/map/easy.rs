mod full_cone;
mod port_restricted_cone;
mod restricted_cone;

pub use full_cone::FullCone;
pub use port_restricted_cone::PortRestrictedCone;
pub use restricted_cone::RestrictedCone;

pub enum Easy {
    Full(FullCone),
    Restricted(RestrictedCone),
    PortRestricted(PortRestrictedCone),
}
