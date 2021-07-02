/// A Marker trait that should not be exposed publicly. It is used to mark a trait as "sealed",
/// which means that the trait cannot be implemented outside of this crate
pub trait Sealed {}
