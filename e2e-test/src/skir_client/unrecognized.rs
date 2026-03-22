use std::marker::PhantomData;
use std::vec::Vec;

/// Stores unrecognized fields encountered while deserializing a struct of type
/// `T`.
pub type UnrecognizedFields<T> = Option<UnrecognizedFieldsData<T>>;

/// Stores unrecognized fields encountered while deserializing an enum of type
/// `T`.
pub type UnrecognizedVariant<T> = Option<UnrecognizedVariantData<T>>;

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
#[derive(Clone, Debug, Default)]
pub struct UnrecognizedVariantData<T> {
    pub(super) format: UnrecognizedFormat,
    /// Wire number of the unrecognized variant.
    pub(super) number: i32,
    /// Present if the variant wraps a value; `None` if it is a plain number.
    pub(super) value: Option<Box<Vec<u8>>>,
    _phantom: PhantomData<T>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) enum UnrecognizedFormat {
    #[default]
    Unknown,
    DenseJson,
    Bytes,
}
