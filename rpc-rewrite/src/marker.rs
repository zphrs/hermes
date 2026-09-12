mod boolean;
pub mod not_applicable;
mod role;

#[expect(private_bounds, reason = "LeafType")]
pub struct Leaf<T: LeafType + ?Sized>(T);

pub(crate) trait LeafType {}

pub struct Loopback;
pub struct Transition;

impl LeafType for Loopback {}
impl LeafType for Transition {}

pub struct CanTransition;
pub struct Branch<T: BranchType + ?Sized>(T);

trait SealedBranchType {}

#[expect(private_bounds, reason = "SealedBranchType")]
pub trait BranchType: SealedBranchType {}

impl<T: SealedBranchType + ?Sized> BranchType for T {}

impl SealedBranchType for Loopback {}
impl SealedBranchType for Transition {}
impl SealedBranchType for CanTransition {}

pub(crate) trait MethodType {}

impl<T: LeafType> MethodType for Leaf<T> {}

impl<T: BranchType> MethodType for Branch<T> {}

pub use boolean::{False, True};
pub use not_applicable::NotApplicable;
pub use role::{Client, Server};
