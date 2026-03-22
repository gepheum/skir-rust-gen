use std::marker::PhantomData;
use std::vec::Vec;

/// Stores unrecognized fields encountered while deserializing a struct of type
/// `T`.
pub type UnrecognizedFields<T> = Box<UnrecognizedFieldsData<T>>;

/// Stores unrecognized fields encountered while deserializing an enum of type
/// `T`.
pub type UnrecognizedVariant<T> = Box<UnrecognizedVariantData<T>>;

/// Internal data owned by `UnrecognizedFields<T>` instances.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnrecognizedFieldsData<T> {
    pub(super) format: UnrecognizedFormat,
    pub(super) array_len: u32,
    // Raw bytes of the unrecognized field values.
    pub(super) values: Vec<u8>,
    _phantom: PhantomData<T>,
}

/// Stores an unrecognized enum variant encountered while deserializing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnrecognizedVariantData<T> {
    pub(super) format: UnrecognizedFormat,
    /// Wire number of the unrecognized variant.
    pub(super) number: i32,
    /// Empty if the unrecognized variant is a constant variant (number).
    pub(super) value: Vec<u8>,
    _phantom: PhantomData<T>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) enum UnrecognizedFormat {
    #[default]
    Unknown,
    DenseJson,
    Bytes,
}
