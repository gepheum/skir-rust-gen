pub mod internal {

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::super::reflection::{
    EnumConstantVariant, EnumDescriptor, EnumVariant, EnumWrapperVariant, TypeDescriptor,
};
use super::super::serializer::{Serializer, TypeAdapter};
use super::super::serializers::{
    decode_number, decode_number_body, encode_uint32, read_u8, skip_value,
};
use super::super::unrecognized::{UnrecognizedFormat, UnrecognizedVariantData};

// =============================================================================
// AnyEntry – maps a variant number to how it should be handled
// =============================================================================

enum AnyEntry {
    Removed,
    Constant(usize), // kind_ordinal
    Wrapper(usize),  // kind_ordinal
}

// =============================================================================
// VariantEntry – type-erased per-variant adapter
// =============================================================================

trait VariantEntry<T> {
    fn kind_ordinal(&self) -> usize;
    fn name(&self) -> &str;
    fn number(&self) -> i32;
    fn is_wrapper(&self) -> bool;
    /// Returns the enum value for a constant variant.
    fn instance(&self) -> T;
    fn to_json(&self, frozen: &T, eol_indent: Option<&str>, out: &mut String);
    fn encode_value(&self, frozen: &T, out: &mut Vec<u8>);
    fn wrap_from_json(&self, v: &serde_json::Value, keep: bool) -> Result<T, String>;
    fn wrap_decode(&self, input: &mut &[u8], keep: bool) -> Result<T, String>;
    fn clone_box(&self) -> Box<dyn VariantEntry<T>>;
}

// =============================================================================
// ConstantEntry
// =============================================================================

struct ConstantEntry<T: 'static> {
    kind_ordinal: usize,
    name: String,
    number: i32,
    instance_fn: fn() -> T,
}

impl<T: 'static> VariantEntry<T> for ConstantEntry<T> {
    fn kind_ordinal(&self) -> usize {
        self.kind_ordinal
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn number(&self) -> i32 {
        self.number
    }
    fn is_wrapper(&self) -> bool {
        false
    }
    fn instance(&self) -> T {
        (self.instance_fn)()
    }

    fn to_json(&self, _frozen: &T, eol_indent: Option<&str>, out: &mut String) {
        if eol_indent.is_some() {
            // Readable: emit the variant name as a JSON string.
            write_json_escaped_string(&self.name, out);
        } else {
            // Dense: emit the variant number as a decimal string.
            out.push_str(&self.number.to_string());
        }
    }

    fn encode_value(&self, _frozen: &T, out: &mut Vec<u8>) {
        encode_uint32(self.number as u32, out);
    }

    fn wrap_from_json(&self, _v: &serde_json::Value, _keep: bool) -> Result<T, String> {
        Err(format!("variant '{}' is a constant, not a wrapper", self.name))
    }

    fn wrap_decode(&self, _input: &mut &[u8], _keep: bool) -> Result<T, String> {
        Err(format!("variant '{}' is a constant, not a wrapper", self.name))
    }

    fn clone_box(&self) -> Box<dyn VariantEntry<T>> {
        Box::new(ConstantEntry {
            kind_ordinal: self.kind_ordinal,
            name: self.name.clone(),
            number: self.number,
            instance_fn: self.instance_fn,
        })
    }
}

// =============================================================================
// WrapperEntry
// =============================================================================

struct WrapperEntry<T: 'static, V: 'static> {
    kind_ordinal: usize,
    name: String,
    number: i32,
    ser: Serializer<V>,
    wrap: fn(V) -> T,
    get_value: fn(&T) -> &V,
}

impl<T: 'static, V: 'static> VariantEntry<T> for WrapperEntry<T, V> {
    fn kind_ordinal(&self) -> usize {
        self.kind_ordinal
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn number(&self) -> i32 {
        self.number
    }
    fn is_wrapper(&self) -> bool {
        true
    }
    fn instance(&self) -> T {
        panic!("instance() called on wrapper variant '{}'", self.name)
    }

    fn to_json(&self, frozen: &T, eol_indent: Option<&str>, out: &mut String) {
        let v = (self.get_value)(frozen);
        if let Some(indent) = eol_indent {
            // Readable: {"kind": "NAME", "value": <value>}
            let child_indent = format!("{indent}  ");
            out.push('{');
            out.push_str(&child_indent);
            out.push_str("\"kind\": ");
            write_json_escaped_string(&self.name, out);
            out.push(',');
            out.push_str(&child_indent);
            out.push_str("\"value\": ");
            self.ser.adapter().to_json(v, Some(&child_indent), out);
            out.push_str(indent);
            out.push('}');
        } else {
            // Dense: [number, value_json]
            out.push('[');
            out.push_str(&self.number.to_string());
            out.push(',');
            self.ser.adapter().to_json(v, None, out);
            out.push(']');
        }
    }

    fn encode_value(&self, frozen: &T, out: &mut Vec<u8>) {
        let v = (self.get_value)(frozen);
        self.ser.adapter().encode(v, out);
    }

    fn wrap_from_json(&self, v: &serde_json::Value, keep: bool) -> Result<T, String> {
        let inner = self.ser.adapter().from_json(v, keep)?;
        Ok((self.wrap)(inner))
    }

    fn wrap_decode(&self, input: &mut &[u8], keep: bool) -> Result<T, String> {
        let inner = self.ser.adapter().decode(input, keep)?;
        Ok((self.wrap)(inner))
    }

    fn clone_box(&self) -> Box<dyn VariantEntry<T>> {
        Box::new(WrapperEntry {
            kind_ordinal: self.kind_ordinal,
            name: self.name.clone(),
            number: self.number,
            ser: self.ser.clone(),
            wrap: self.wrap,
            get_value: self.get_value,
        })
    }
}

// =============================================================================
// EnumAdapter
// =============================================================================

/// Implements [`TypeAdapter<T>`] for a Skir enum type.
///
/// For use only by code generated by the Skir Rust code generator.
///
/// - `T` is the enum type.
///
/// Usage: call [`EnumAdapter::new`], then register variants with
/// [`EnumAdapter::add_constant_variant`] / [`EnumAdapter::add_wrapper_variant`] /
/// [`EnumAdapter::add_removed_number`], then call
/// [`EnumAdapter::into_serializer`].
pub struct EnumAdapter<T: 'static> {
    get_kind_ordinal: fn(&T) -> usize,
    unknown_fn: fn() -> T,
    wrap_unrecognized: fn(Box<UnrecognizedVariantData<T>>) -> T,
    get_unrecognized: fn(&T) -> Option<&UnrecognizedVariantData<T>>,
    /// Maps variant number → how to handle it (removed / constant / wrapper).
    number_to_entry: HashMap<i32, AnyEntry>,
    /// Maps variant name → kind_ordinal (for known, non-removed variants).
    name_to_kind_ordinal: HashMap<String, usize>,
    /// Indexed by kind_ordinal. Index 0 is always None (UNKNOWN pseudo-entry).
    kind_ordinal_to_entry: Vec<Option<Box<dyn VariantEntry<T>>>>,
    /// Accumulates variants to pass to the descriptor on finalize.
    desc_variants: Vec<EnumVariant>,
    /// Pre-allocated descriptor so [`descriptor()`] is valid before finalization.
    desc: Arc<EnumDescriptor>,
}

impl<T: 'static> EnumAdapter<T> {
    /// Creates a new `EnumAdapter`.
    pub fn new(
        get_kind_ordinal: fn(&T) -> usize,
        unknown_fn: fn() -> T,
        wrap_unrecognized: fn(Box<UnrecognizedVariantData<T>>) -> T,
        get_unrecognized: fn(&T) -> Option<&UnrecognizedVariantData<T>>,
        module_path: &str,
        qualified_name: &str,
        doc: &str,
        removed_numbers: HashSet<i32>,
    ) -> Self {
        let desc = Arc::new(EnumDescriptor::new(
            module_path.to_string(),
            qualified_name.to_string(),
            doc.to_string(),
            removed_numbers,
        ));
        // Slot 0 is reserved for UNKNOWN (kind_ordinal 0 → no variant entry).
        let kind_ordinal_to_entry = vec![None];
        EnumAdapter {
            get_kind_ordinal,
            unknown_fn,
            wrap_unrecognized,
            get_unrecognized,
            number_to_entry: HashMap::new(),
            name_to_kind_ordinal: HashMap::new(),
            kind_ordinal_to_entry,
            desc_variants: Vec::new(),
            desc,
        }
    }

    /// Registers a constant (non-wrapping) enum variant.
    pub fn add_constant_variant(
        &mut self,
        name: &str,
        number: i32,
        kind_ordinal: usize,
        doc: &str,
        instance_fn: fn() -> T,
    ) {
        self.number_to_entry.insert(number, AnyEntry::Constant(kind_ordinal));
        self.name_to_kind_ordinal.insert(name.to_string(), kind_ordinal);
        let entry: Box<dyn VariantEntry<T>> = Box::new(ConstantEntry {
            kind_ordinal,
            name: name.to_string(),
            number,
            instance_fn,
        });
        self.set_kind_ordinal_entry(kind_ordinal, entry);
        self.desc_variants.push(EnumVariant::Constant(EnumConstantVariant::new(
            name.to_string(),
            number,
            doc.to_string(),
        )));
    }

    /// Registers a wrapper enum variant.
    pub fn add_wrapper_variant<V: 'static>(
        &mut self,
        name: &str,
        number: i32,
        kind_ordinal: usize,
        ser: Serializer<V>,
        doc: &str,
        wrap: fn(V) -> T,
        get_value: fn(&T) -> &V,
    ) {
        let type_desc = ser.adapter().type_descriptor();
        self.number_to_entry.insert(number, AnyEntry::Wrapper(kind_ordinal));
        self.name_to_kind_ordinal.insert(name.to_string(), kind_ordinal);
        let entry: Box<dyn VariantEntry<T>> = Box::new(WrapperEntry {
            kind_ordinal,
            name: name.to_string(),
            number,
            ser,
            wrap,
            get_value,
        });
        self.set_kind_ordinal_entry(kind_ordinal, entry);
        self.desc_variants.push(EnumVariant::Wrapper(EnumWrapperVariant::new(
            name.to_string(),
            number,
            type_desc,
            doc.to_string(),
        )));
    }

    /// Registers a variant number that was removed from the schema.
    pub fn add_removed_number(&mut self, number: i32) {
        self.number_to_entry.insert(number, AnyEntry::Removed);
    }

    fn set_kind_ordinal_entry(&mut self, kind_ordinal: usize, entry: Box<dyn VariantEntry<T>>) {
        if kind_ordinal >= self.kind_ordinal_to_entry.len() {
            self.kind_ordinal_to_entry.resize_with(kind_ordinal + 1, || None);
        }
        self.kind_ordinal_to_entry[kind_ordinal] = Some(entry);
    }

    fn finalize(&mut self) {
        self.desc_variants.sort_by_key(|v| v.number());
        let variants = std::mem::take(&mut self.desc_variants);
        self.desc.set_variants(variants);
    }

    /// Finalizes the adapter and returns a [`Serializer<T>`] backed by it.
    pub fn into_serializer(mut self) -> Serializer<T> {
        self.finalize();
        Serializer::new(EnumAdapterWrapper(Arc::new(self)))
    }

    /// Returns a reference to the pre-allocated [`EnumDescriptor`] for this
    /// adapter. Valid even before [`into_serializer`](Self::into_serializer) is
    /// called, which is necessary for recursive enum variant types.
    pub fn descriptor(&self) -> Arc<EnumDescriptor> {
        Arc::clone(&self.desc)
    }

    // -----------------------------------------------------------------------
    // TypeAdapter implementation helpers
    // -----------------------------------------------------------------------

    fn is_default_impl(&self, input: &T) -> bool {
        (self.get_kind_ordinal)(input) == 0
    }

    fn to_json_impl(&self, input: &T, eol_indent: Option<&str>, out: &mut String) {
        let ko = (self.get_kind_ordinal)(input);
        if ko == 0 {
            self.unknown_to_json(input, eol_indent, out);
            return;
        }
        if let Some(Some(entry)) = self.kind_ordinal_to_entry.get(ko) {
            entry.to_json(input, eol_indent, out);
        } else {
            // Fallback: shouldn't happen with well-formed generated code.
            if eol_indent.is_some() {
                out.push_str("\"UNKNOWN\"");
            } else {
                out.push('0');
            }
        }
    }

    fn unknown_to_json(&self, input: &T, eol_indent: Option<&str>, out: &mut String) {
        if eol_indent.is_some() {
            out.push_str("\"UNKNOWN\"");
            return;
        }
        // Dense: emit stored JSON if available, else fall back to "0".
        if let Some(u) = (self.get_unrecognized)(input) {
            if u.format == UnrecognizedFormat::DenseJson && !u.value.is_empty() {
                out.push_str(std::str::from_utf8(&u.value).unwrap_or("0"));
                return;
            }
        }
        out.push('0');
    }

    fn from_json_impl(&self, v: &serde_json::Value, keep: bool) -> Result<T, String> {
        match v {
            serde_json::Value::Number(n) => {
                let num = n.as_i64().unwrap_or(0) as i32;
                Ok(self.resolve_constant_lookup(num, keep, Some(v)))
            }
            serde_json::Value::Bool(b) => {
                let num = if *b { 1 } else { 0 };
                Ok(self.resolve_constant_lookup(num, keep, Some(v)))
            }
            serde_json::Value::String(s) => {
                match self.name_to_kind_ordinal.get(s.as_str()) {
                    None => Ok((self.unknown_fn)()),
                    Some(&ko) => {
                        if let Some(Some(entry)) = self.kind_ordinal_to_entry.get(ko) {
                            if entry.is_wrapper() {
                                return Err(format!(
                                    "variant '{}' is a wrapper, expected a constant",
                                    s
                                ));
                            }
                            Ok(entry.instance())
                        } else {
                            Ok((self.unknown_fn)())
                        }
                    }
                }
            }
            serde_json::Value::Array(arr) if arr.len() == 2 => {
                let num = arr[0].as_i64().unwrap_or(0) as i32;
                match self.number_to_entry.get(&num) {
                    None | Some(AnyEntry::Removed) => Ok((self.unknown_fn)()),
                    Some(AnyEntry::Constant(_)) => Err(format!(
                        "variant number {} is a constant, not a wrapper",
                        num
                    )),
                    Some(AnyEntry::Wrapper(ko)) => {
                        let ko = *ko;
                        if let Some(Some(entry)) = self.kind_ordinal_to_entry.get(ko) {
                            entry.wrap_from_json(&arr[1], keep)
                        } else {
                            Ok((self.unknown_fn)())
                        }
                    }
                }
            }
            serde_json::Value::Object(obj) => {
                let name = obj.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                let val_json = obj.get("value").unwrap_or(&serde_json::Value::Null);
                match self.name_to_kind_ordinal.get(name) {
                    None => Ok((self.unknown_fn)()),
                    Some(&ko) => {
                        if let Some(Some(entry)) = self.kind_ordinal_to_entry.get(ko) {
                            if !entry.is_wrapper() {
                                return Err(format!(
                                    "variant '{}' is a constant, not a wrapper",
                                    name
                                ));
                            }
                            entry.wrap_from_json(val_json, keep)
                        } else {
                            Ok((self.unknown_fn)())
                        }
                    }
                }
            }
            _ => Ok((self.unknown_fn)()),
        }
    }

    /// Resolves a variant number seen in a constant context (JSON number or
    /// binary wire < 242).  If unrecognized and `keep` is true, wraps the raw
    /// representation; otherwise returns UNKNOWN.
    fn resolve_constant_lookup(
        &self,
        number: i32,
        keep: bool,
        raw_json: Option<&serde_json::Value>,
    ) -> T {
        match self.number_to_entry.get(&number) {
            None => {
                if keep {
                    let ud = if let Some(v) = raw_json {
                        let bytes = serde_json::to_vec(v).unwrap_or_default();
                        UnrecognizedVariantData::new_from_json(number, bytes)
                    } else {
                        let mut bytes = Vec::new();
                        encode_uint32(number as u32, &mut bytes);
                        UnrecognizedVariantData::new_from_bytes(number, bytes)
                    };
                    (self.wrap_unrecognized)(ud)
                } else {
                    (self.unknown_fn)()
                }
            }
            Some(AnyEntry::Removed) => (self.unknown_fn)(),
            // A wrapper variant encountered in a constant context is an error;
            // return UNKNOWN.
            Some(AnyEntry::Wrapper(_)) => (self.unknown_fn)(),
            Some(AnyEntry::Constant(ko)) => {
                let ko = *ko;
                if let Some(Some(entry)) = self.kind_ordinal_to_entry.get(ko) {
                    entry.instance()
                } else {
                    (self.unknown_fn)()
                }
            }
        }
    }

    fn encode_impl(&self, input: &T, out: &mut Vec<u8>) {
        let ko = (self.get_kind_ordinal)(input);
        if ko == 0 {
            // UNKNOWN: carry through raw wire bytes if available.
            if let Some(u) = (self.get_unrecognized)(input) {
                if u.format == UnrecognizedFormat::Bytes && !u.value.is_empty() {
                    out.extend_from_slice(&u.value);
                    return;
                }
            }
            out.push(0);
            return;
        }
        if let Some(Some(entry)) = self.kind_ordinal_to_entry.get(ko) {
            if !entry.is_wrapper() {
                // Constant variant: encoded as a variable-length uint32.
                encode_uint32(entry.number() as u32, out);
            } else {
                // Wrapper variant: header byte(s) followed by the wrapped value.
                let n = entry.number();
                if n >= 1 && n <= 4 {
                    out.push(250 + n as u8);
                } else {
                    out.push(248);
                    encode_uint32(n as u32, out);
                }
                entry.encode_value(input, out);
            }
        } else {
            out.push(0); // fallback
        }
    }

    fn decode_impl(&self, input: &mut &[u8], keep: bool) -> Result<T, String> {
        let wire = read_u8(input)?;

        if wire < 242 {
            // Constant variant: decode the number from the wire byte.
            let n = decode_number_body(wire, input)? as i32;
            return Ok(self.resolve_constant_lookup(n, keep, None));
        }

        // Wrapper variant: determine the variant number.
        let number: i32 = if wire == 248 {
            decode_number(input)? as i32
        } else if wire >= 251 && wire <= 254 {
            (wire - 250) as i32
        } else {
            // Unknown wire byte (e.g. 242..247, 249, 250, 255): skip nothing and
            // return UNKNOWN.
            return Ok((self.unknown_fn)());
        };

        match self.number_to_entry.get(&number) {
            Some(AnyEntry::Wrapper(ko)) => {
                let ko = *ko;
                if let Some(Some(entry)) = self.kind_ordinal_to_entry.get(ko) {
                    entry.wrap_decode(input, keep)
                } else {
                    skip_value(input)?;
                    Ok((self.unknown_fn)())
                }
            }
            Some(AnyEntry::Removed) => {
                skip_value(input)?;
                Ok((self.unknown_fn)())
            }
            // Not found or unexpectedly maps to a constant: treat as an
            // unrecognized wrapper number.
            None | Some(AnyEntry::Constant(_)) => {
                if keep {
                    // Re-encode the header we already consumed so the full wire
                    // representation can be round-tripped.
                    let mut header = Vec::new();
                    if number >= 1 && number <= 4 {
                        header.push(250 + number as u8);
                    } else {
                        header.push(248);
                        encode_uint32(number as u32, &mut header);
                    }
                    let before = *input;
                    let before_len = before.len();
                    skip_value(input)?;
                    let consumed = before_len - input.len();
                    let mut all_bytes = header;
                    all_bytes.extend_from_slice(&before[..consumed]);
                    Ok((self.wrap_unrecognized)(UnrecognizedVariantData::new_from_bytes(
                        number, all_bytes,
                    )))
                } else {
                    skip_value(input)?;
                    Ok((self.unknown_fn)())
                }
            }
        }
    }

    fn type_descriptor_impl(&self) -> TypeDescriptor {
        TypeDescriptor::Enum(Arc::clone(&self.desc))
    }
}

// =============================================================================
// EnumAdapterWrapper – cheap Arc clone for TypeAdapter::clone_box
// =============================================================================

struct EnumAdapterWrapper<T: 'static>(Arc<EnumAdapter<T>>);

impl<T: 'static> TypeAdapter<T> for EnumAdapterWrapper<T> {
    fn is_default(&self, input: &T) -> bool {
        self.0.is_default_impl(input)
    }

    fn to_json(&self, input: &T, eol_indent: Option<&str>, out: &mut String) {
        self.0.to_json_impl(input, eol_indent, out);
    }

    fn from_json(
        &self,
        json: &serde_json::Value,
        keep_unrecognized: bool,
    ) -> Result<T, String> {
        self.0.from_json_impl(json, keep_unrecognized)
    }

    fn encode(&self, input: &T, out: &mut Vec<u8>) {
        self.0.encode_impl(input, out);
    }

    fn decode(&self, input: &mut &[u8], keep_unrecognized: bool) -> Result<T, String> {
        self.0.decode_impl(input, keep_unrecognized)
    }

    fn type_descriptor(&self) -> TypeDescriptor {
        self.0.type_descriptor_impl()
    }

    fn clone_box(&self) -> Box<dyn TypeAdapter<T>> {
        Box::new(EnumAdapterWrapper(Arc::clone(&self.0)))
    }
}

// =============================================================================
// JSON helpers
// =============================================================================

fn write_json_escaped_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

} // pub mod internal

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::internal::*;
    use crate::skir_client::reflection::TypeDescriptor;
    use crate::skir_client::serializer::Serializer;
    use crate::skir_client::serializers::int32_serializer;
    use crate::skir_client::unrecognized::{UnrecognizedFormat, UnrecognizedVariantData};

    // -------------------------------------------------------------------------
    // A minimal test enum
    // -------------------------------------------------------------------------
    //   UNKNOWN = kind_ordinal 0  (not a registered variant)
    //   RED     = kind_ordinal 1, number 1  (constant)
    //   GREEN   = kind_ordinal 2, number 2  (constant)
    //   WRAPPED = kind_ordinal 3, number 3  (wrapper around i32)
    //
    // Wire encoding reminder:
    //   RED    → encode_uint32(1) → wire byte 0x01
    //   GREEN  → encode_uint32(2) → wire byte 0x02
    //   WRAPPED(v) → wire byte 0xFD (250+3), then int32 encoding of v

    #[derive(Clone, Debug, PartialEq)]
    enum Color {
        Unknown,
        Red,
        Green,
        Wrapped(i32),
        Unrecognized(Box<UnrecognizedVariantData<Color>>),
    }

    impl Default for Color {
        fn default() -> Self {
            Color::Unknown
        }
    }

    static DEFAULT_WRAPPED_VALUE: i32 = 0;

    fn color_kind_ordinal(c: &Color) -> usize {
        match c {
            Color::Unknown | Color::Unrecognized(_) => 0,
            Color::Red => 1,
            Color::Green => 2,
            Color::Wrapped(_) => 3,
        }
    }

    fn color_unknown() -> Color {
        Color::Unknown
    }

    fn color_wrap_unrecognized(u: Box<UnrecognizedVariantData<Color>>) -> Color {
        Color::Unrecognized(u)
    }

    fn color_get_unrecognized(c: &Color) -> Option<&UnrecognizedVariantData<Color>> {
        match c {
            Color::Unrecognized(u) => Some(u.as_ref()),
            _ => None,
        }
    }

    fn red_fn() -> Color {
        Color::Red
    }
    fn green_fn() -> Color {
        Color::Green
    }
    fn wrap_int32(v: i32) -> Color {
        Color::Wrapped(v)
    }
    fn get_wrapped_value(c: &Color) -> &i32 {
        match c {
            Color::Wrapped(v) => v,
            _ => &DEFAULT_WRAPPED_VALUE,
        }
    }

    fn make_color_serializer() -> Serializer<Color> {
        let mut a = EnumAdapter::new(
            color_kind_ordinal,
            color_unknown,
            color_wrap_unrecognized,
            color_get_unrecognized,
            "test",
            "Color",
            "A test color enum.",
            HashSet::new(),
        );
        a.add_constant_variant("RED", 1, 1, "The color red.", red_fn);
        a.add_constant_variant("GREEN", 2, 2, "The color green.", green_fn);
        a.add_wrapper_variant(
            "WRAPPED",
            3,
            3,
            int32_serializer(),
            "A wrapped integer.",
            wrap_int32,
            get_wrapped_value,
        );
        a.into_serializer()
    }

    // -------------------------------------------------------------------------
    // is_default
    // -------------------------------------------------------------------------

    #[test]
    fn is_default_for_unknown() {
        let s = make_color_serializer();
        assert!(s.adapter().is_default(&Color::Unknown));
    }

    #[test]
    fn is_not_default_for_red() {
        let s = make_color_serializer();
        assert!(!s.adapter().is_default(&Color::Red));
    }

    #[test]
    fn is_not_default_for_wrapped() {
        let s = make_color_serializer();
        assert!(!s.adapter().is_default(&Color::Wrapped(0)));
    }

    // -------------------------------------------------------------------------
    // Dense JSON encoding
    // -------------------------------------------------------------------------

    #[test]
    fn dense_json_unknown_is_zero() {
        let s = make_color_serializer();
        assert_eq!(s.to_json(&Color::Unknown, false), "0");
    }

    #[test]
    fn dense_json_constant_emits_number() {
        let s = make_color_serializer();
        assert_eq!(s.to_json(&Color::Red, false), "1");
        assert_eq!(s.to_json(&Color::Green, false), "2");
    }

    #[test]
    fn dense_json_wrapper_emits_array() {
        let s = make_color_serializer();
        assert_eq!(s.to_json(&Color::Wrapped(42), false), "[3,42]");
    }

    // -------------------------------------------------------------------------
    // Readable JSON encoding
    // -------------------------------------------------------------------------

    #[test]
    fn readable_json_unknown_is_string() {
        let s = make_color_serializer();
        assert_eq!(s.to_json(&Color::Unknown, true), "\"UNKNOWN\"");
    }

    #[test]
    fn readable_json_constant_emits_name() {
        let s = make_color_serializer();
        assert_eq!(s.to_json(&Color::Red, true), "\"RED\"");
        assert_eq!(s.to_json(&Color::Green, true), "\"GREEN\"");
    }

    #[test]
    fn readable_json_wrapper_emits_object() {
        let s = make_color_serializer();
        let json = s.to_json(&Color::Wrapped(7), true);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["kind"], "WRAPPED");
        assert_eq!(v["value"], 7);
    }

    // -------------------------------------------------------------------------
    // JSON round-trips
    // -------------------------------------------------------------------------

    #[test]
    fn dense_json_round_trip_constant() {
        let s = make_color_serializer();
        for c in [Color::Unknown, Color::Red, Color::Green] {
            let json = s.to_json(&c, false);
            let c2 = s.from_json(&json, false).unwrap();
            assert_eq!(c, c2, "round-trip failed for {json}");
        }
    }

    #[test]
    fn dense_json_round_trip_wrapper() {
        let s = make_color_serializer();
        let c = Color::Wrapped(-5);
        let json = s.to_json(&c, false);
        assert_eq!(s.from_json(&json, false).unwrap(), c);
    }

    #[test]
    fn readable_json_round_trip_constant() {
        let s = make_color_serializer();
        let json = s.to_json(&Color::Red, true);
        assert_eq!(s.from_json(&json, false).unwrap(), Color::Red);
    }

    #[test]
    fn readable_json_round_trip_wrapper() {
        let s = make_color_serializer();
        let c = Color::Wrapped(99);
        let json = s.to_json(&c, true);
        assert_eq!(s.from_json(&json, false).unwrap(), c);
    }

    #[test]
    fn from_json_bool_true_maps_to_number_1() {
        let s = make_color_serializer();
        // Red has number 1, so `true` should decode as Red.
        let c = s.from_json("true", false).unwrap();
        assert_eq!(c, Color::Red);
    }

    #[test]
    fn from_json_bool_false_maps_to_number_0() {
        let s = make_color_serializer();
        // No variant has number 0, so false → Unknown.
        let c = s.from_json("false", false).unwrap();
        assert_eq!(c, Color::Unknown);
    }

    // -------------------------------------------------------------------------
    // Binary round-trips
    // -------------------------------------------------------------------------

    fn binary_round_trip(c: &Color) -> Color {
        let s = make_color_serializer();
        let bytes = s.to_bytes(c);
        s.from_bytes(&bytes, false).unwrap()
    }

    #[test]
    fn binary_round_trip_unknown() {
        let c = Color::Unknown;
        assert_eq!(binary_round_trip(&c), c);
    }

    #[test]
    fn binary_round_trip_constant() {
        assert_eq!(binary_round_trip(&Color::Red), Color::Red);
        assert_eq!(binary_round_trip(&Color::Green), Color::Green);
    }

    #[test]
    fn binary_round_trip_wrapper() {
        let c = Color::Wrapped(123);
        assert_eq!(binary_round_trip(&c), c);
    }

    #[test]
    fn binary_unknown_encodes_as_wire_0() {
        let s = make_color_serializer();
        let bytes = s.to_bytes(&Color::Unknown);
        assert_eq!(&bytes[4..], &[0u8]);
    }

    #[test]
    fn binary_red_encodes_as_wire_1() {
        let s = make_color_serializer();
        let bytes = s.to_bytes(&Color::Red);
        // encode_uint32(1) → single byte 0x01
        assert_eq!(&bytes[4..], &[1u8]);
    }

    #[test]
    fn binary_wrapped_uses_short_wire_header() {
        let s = make_color_serializer();
        let bytes = s.to_bytes(&Color::Wrapped(0));
        // number 3 is in [1..4] so wire byte = 250 + 3 = 253 = 0xFD
        assert_eq!(bytes[4], 253u8);
    }

    // -------------------------------------------------------------------------
    // Removed variant numbers
    // -------------------------------------------------------------------------

    fn make_color_serializer_with_removed() -> Serializer<Color> {
        let mut a = EnumAdapter::new(
            color_kind_ordinal,
            color_unknown,
            color_wrap_unrecognized,
            color_get_unrecognized,
            "test",
            "Color",
            "",
            HashSet::new(),
        );
        a.add_constant_variant("RED", 1, 1, "", red_fn);
        a.add_removed_number(5); // variant 5 was removed
        a.into_serializer()
    }

    #[test]
    fn binary_removed_constant_decodes_as_unknown() {
        let s = make_color_serializer_with_removed();
        // encode_uint32(5) → wire byte 5
        let payload: Vec<u8> = {
            let mut v = b"skir".to_vec();
            v.push(5);
            v
        };
        let c = s.from_bytes(&payload, false).unwrap();
        assert_eq!(c, Color::Unknown);
    }

    // -------------------------------------------------------------------------
    // Unrecognized variant carry-through – binary
    // -------------------------------------------------------------------------

    #[test]
    fn binary_unrecognized_constant_carry_through() {
        let s = make_color_serializer();
        // Wire byte 99 → number 99 (unrecognized constant).
        let payload: Vec<u8> = {
            let mut v = b"skir".to_vec();
            v.push(99);
            v
        };
        // Decode with keep=true → should produce Unrecognized variant.
        let c = s.from_bytes(&payload, true).unwrap();
        match &c {
            Color::Unrecognized(u) => {
                assert_eq!(u.number, 99);
                assert_eq!(u.format, UnrecognizedFormat::Bytes);
            }
            _ => panic!("expected Unrecognized, got {:?}", c),
        }
        // Re-encode must produce the same payload.
        let reencoded = s.to_bytes(&c);
        assert_eq!(reencoded, payload);
    }

    #[test]
    fn binary_unrecognized_wrapper_carry_through() {
        let s = make_color_serializer();
        // Wrapper number 7 (short form: 250+7 overflows byte... 7 > 4, use long form)
        // Wire: 0xF8 = 248, then encode_uint32(7) = 0x07, then inner value = 0 (int32 zero)
        let payload: Vec<u8> = {
            let mut v = b"skir".to_vec();
            v.push(248); // long-form wrapper header
            v.push(7);   // number = 7
            v.push(0);   // inner int32 zero = wire byte 0
            v
        };
        let c = s.from_bytes(&payload, true).unwrap();
        match &c {
            Color::Unrecognized(u) => {
                assert_eq!(u.number, 7);
                assert_eq!(u.format, UnrecognizedFormat::Bytes);
                // value should contain the header (248, 7) plus the inner value (0)
                assert_eq!(u.value, &[248u8, 7, 0]);
            }
            _ => panic!("expected Unrecognized, got {:?}", c),
        }
        // Re-encode must reproduce the original payload.
        let reencoded = s.to_bytes(&c);
        assert_eq!(reencoded, payload);
    }

    // -------------------------------------------------------------------------
    // Unrecognized variant carry-through – JSON
    // -------------------------------------------------------------------------

    #[test]
    fn json_unrecognized_constant_carry_through() {
        let s = make_color_serializer();
        // Dense JSON: number 99 is not a known variant.
        let c = s.from_json("99", true).unwrap();
        match &c {
            Color::Unrecognized(u) => {
                assert_eq!(u.number, 99);
                assert_eq!(u.format, UnrecognizedFormat::DenseJson);
            }
            _ => panic!("expected Unrecognized, got {:?}", c),
        }
        // Re-serialize: should emit the stored JSON bytes.
        let out = s.to_json(&c, false);
        assert_eq!(out, "99");
    }

    // -------------------------------------------------------------------------
    // TypeDescriptor / reflection
    // -------------------------------------------------------------------------

    #[test]
    fn type_descriptor_is_enum() {
        let s = make_color_serializer();
        match s.type_descriptor() {
            TypeDescriptor::Enum(desc) => {
                assert_eq!(desc.qualified_name(), "Color");
                assert_eq!(desc.variants().len(), 3);
                // Variants are sorted by number.
                assert_eq!(desc.variants()[0].name(), "RED");
                assert_eq!(desc.variants()[1].name(), "GREEN");
                assert_eq!(desc.variants()[2].name(), "WRAPPED");
            }
            _ => panic!("expected TypeDescriptor::Enum"),
        }
    }
}
