use std::marker::PhantomData;
use std::vec::Vec;

/// Stores unrecognized fields encountered while deserializing a struct of type
/// `T`. The type parameter is a phantom: it is never stored, but it prevents
/// accidentally assigning an `UnrecognizedFields<Foo>` to an
/// `UnrecognizedFields<Bar>`.
#[derive(Clone, Debug, Default)]
pub struct UnrecognizedFields<T> {
    pub(super) data: Option<Box<UnrecognizedFieldsData>>,
    _phantom: PhantomData<T>,
}

impl<T> UnrecognizedFields<T> {
    pub(super) fn new() -> Self {
        Self {
            data: None,
            _phantom: PhantomData,
        }
    }
}

/// Internal data shared by `UnrecognizedFields<T>` instances.
#[derive(Clone, Debug, Default)]
pub(super) struct UnrecognizedFieldsData {
    pub format: UnrecognizedFormat,
    pub array_len: u32,
    // Raw bytes of the unrecognized field values.
    pub values: Vec<u8>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub(super) enum UnrecognizedFormat {
    #[default]
    Unknown,
    DenseJson,
    Bytes,
}

/// Stores an unrecognized enum variant encountered while deserializing.
#[derive(Clone, Debug, Default)]
pub struct UnrecognizedVariant {
    pub format: UnrecognizedFormat,
    /// Wire number of the unrecognized variant.
    pub number: i32,
    /// Present if the variant wraps a value; `None` if it is a plain number.
    pub value: Option<Box<Vec<u8>>>,
}

impl UnrecognizedVariant {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets `value` to a fresh empty buffer and returns a mutable reference to
    /// it, mirroring the C++ `emplace_value()` method.
    pub fn emplace_value(&mut self) -> &mut Vec<u8> {
        self.value = Some(Box::new(Vec::new()));
        self.value.as_mut().unwrap()
    }
}
