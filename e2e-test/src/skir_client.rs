pub mod keyed_vec;
pub mod reflection;
pub mod serializer;
pub mod serializers;
pub mod enum_adapter;
pub mod struct_adapter;
pub mod unrecognized;

// =============================================================================
// Re-exports
// =============================================================================

// keyed_vec
pub use keyed_vec::KeyedVec;
pub use keyed_vec::KeyedVecSpec;

// serializer
pub use serializer::Serializer;

// serializers
pub use serializers::array_serializer;
pub use serializers::bool_serializer;
pub use serializers::bytes_serializer;
pub use serializers::float32_serializer;
pub use serializers::float64_serializer;
pub use serializers::hash64_serializer;
pub use serializers::int32_serializer;
pub use serializers::int64_serializer;
pub use serializers::keyed_array_serializer;
pub use serializers::optional_serializer;
pub use serializers::string_serializer;
pub use serializers::timestamp_serializer;

/// Items from the `internal` modules of sub-modules, re-exported as a single
/// top-level `internal` module.
pub mod internal {
    // keyed_vec::internal
    pub use super::keyed_vec::internal::BorrowLookup;
    pub use super::keyed_vec::internal::CopyLookup;
    pub use super::keyed_vec::internal::Lookup;

    // enum_adapter::internal
    pub use super::serializers::internal::recursive_serializer;

    // struct_adapter::internal
    pub use super::struct_adapter::internal::StructAdapter;
    pub use super::struct_adapter::internal::struct_serializer_from_static;

    // enum_adapter::internal
    pub use super::enum_adapter::internal::EnumAdapter;
    pub use super::enum_adapter::internal::enum_serializer_from_static;

    // unrecognized::internal
    pub use super::unrecognized::internal::UnrecognizedFields;
    pub use super::unrecognized::internal::UnrecognizedFieldsData;
    pub use super::unrecognized::internal::UnrecognizedFormat;
    pub use super::unrecognized::internal::UnrecognizedVariant;
    pub use super::unrecognized::internal::UnrecognizedVariantData;
}
