use std::time::{Duration, SystemTime};

use super::keyed_vec::{KeyedVec, KeyedVecSpec};
use super::reflection::{ArrayDescriptor, PrimitiveType, TypeDescriptor};
use super::serializer::{Serializer, TypeAdapter};

// =============================================================================
// Public serializer constructors
// =============================================================================

/// Returns a [`Serializer`] for `bool` values.
pub fn bool_serializer() -> Serializer<bool> {
    Serializer::new(BoolAdapter)
}

/// Returns a [`Serializer`] for `i32` values.
pub fn int32_serializer() -> Serializer<i32> {
    Serializer::new(Int32Adapter)
}

/// Returns a [`Serializer`] for `i64` values.
pub fn int64_serializer() -> Serializer<i64> {
    Serializer::new(Int64Adapter)
}

/// Returns a [`Serializer`] for `u64` hash values.
pub fn hash64_serializer() -> Serializer<u64> {
    Serializer::new(Hash64Adapter)
}

/// Returns a [`Serializer`] for `f32` values.
pub fn float32_serializer() -> Serializer<f32> {
    Serializer::new(Float32Adapter)
}

/// Returns a [`Serializer`] for `f64` values.
pub fn float64_serializer() -> Serializer<f64> {
    Serializer::new(Float64Adapter)
}

/// Returns a [`Serializer`] for [`SystemTime`] values (unix-millisecond timestamps).
pub fn timestamp_serializer() -> Serializer<SystemTime> {
    Serializer::new(TimestampAdapter)
}

/// Returns a [`Serializer`] for `String` values.
pub fn string_serializer() -> Serializer<String> {
    Serializer::new(StringAdapter)
}

/// Returns a [`Serializer`] for `Vec<u8>` (bytes) values.
pub fn bytes_serializer() -> Serializer<Vec<u8>> {
    Serializer::new(BytesAdapter)
}

/// Returns a [`Serializer`] for `Vec<T>` arrays.
pub fn array_serializer<T: 'static>(item: Serializer<T>) -> Serializer<Vec<T>> {
    Serializer::new(ArrayAdapter { item })
}

/// Returns a [`Serializer`] for [`KeyedVec<S>`] arrays.
pub fn keyed_array_serializer<S: KeyedVecSpec + 'static>(
    item: Serializer<S::Item>,
) -> Serializer<KeyedVec<S>> {
    Serializer::new(KeyedArrayAdapter { item })
}

/// Returns a [`Serializer`] for `Option<T>` values.
///
/// `None` → JSON `null` / wire `0xff`; `Some(v)` delegates to the inner serializer.
pub fn optional_serializer<T: 'static>(other: Serializer<T>) -> Serializer<Option<T>> {
    Serializer::new(OptionalAdapter { other })
}

pub mod internal {
    use super::{RecursiveAdapter, Serializer};

    /// Returns a [`Serializer`] for hard-recursive optional fields.
    ///
    /// `None` encodes as `[]` (JSON) / wire `0xf6`; `Some(v)` delegates to
    /// `other`.
    pub fn recursive_serializer<T: 'static>(other: Serializer<T>) -> Serializer<Option<T>> {
        Serializer::new(RecursiveAdapter { other })
    }
}

// =============================================================================
// Binary I/O helpers
// =============================================================================

pub(super) fn read_u8(input: &mut &[u8]) -> Result<u8, String> {
    match input.first() {
        Some(&b) => {
            *input = &input[1..];
            Ok(b)
        }
        None => Err("unexpected end of input".to_string()),
    }
}

fn read_u16(input: &mut &[u8]) -> Result<u16, String> {
    if input.len() < 2 {
        return Err("unexpected end of input".to_string());
    }
    let v = u16::from_le_bytes([input[0], input[1]]);
    *input = &input[2..];
    Ok(v)
}

fn read_u32(input: &mut &[u8]) -> Result<u32, String> {
    if input.len() < 4 {
        return Err("unexpected end of input".to_string());
    }
    let v = u32::from_le_bytes([input[0], input[1], input[2], input[3]]);
    *input = &input[4..];
    Ok(v)
}

fn read_i32(input: &mut &[u8]) -> Result<i32, String> {
    read_u32(input).map(|v| v as i32)
}

fn read_u64(input: &mut &[u8]) -> Result<u64, String> {
    if input.len() < 8 {
        return Err("unexpected end of input".to_string());
    }
    let v = u64::from_le_bytes(input[..8].try_into().unwrap());
    *input = &input[8..];
    Ok(v)
}

fn read_i64(input: &mut &[u8]) -> Result<i64, String> {
    read_u64(input).map(|v| v as i64)
}

fn read_f32(input: &mut &[u8]) -> Result<f32, String> {
    read_u32(input).map(f32::from_bits)
}

fn read_f64(input: &mut &[u8]) -> Result<f64, String> {
    read_u64(input).map(f64::from_bits)
}

/// Decodes the body of a variable-length number given the already-consumed wire
/// byte.
pub(super) fn decode_number_body(wire: u8, input: &mut &[u8]) -> Result<i64, String> {
    match wire {
        0..=231 => Ok(wire as i64),
        232 => Ok(read_u16(input)? as i64),
        233 => Ok(read_u32(input)? as i64),
        234 => read_u64(input).map(|v| v as i64), // reinterpret bits
        235 => Ok(read_u8(input)? as i64 - 256),
        236 => Ok(read_u16(input)? as i64 - 65536),
        237 => Ok(read_i32(input)? as i64),
        238 | 239 => read_i64(input),
        240 => read_f32(input).map(|f| f.trunc() as i64),
        241 => read_f64(input).map(|f| f.trunc() as i64),
        _ => Ok(0),
    }
}

/// Reads and decodes the next variable-length number.
pub(super) fn decode_number(input: &mut &[u8]) -> Result<i64, String> {
    let wire = read_u8(input)?;
    decode_number_body(wire, input)
}

/// Encodes an `i32` using the skir variable-length wire format.
fn encode_i32(v: i32, out: &mut Vec<u8>) {
    match v {
        i32::MIN..=-65537 => {
            out.push(237);
            out.extend_from_slice(&v.to_le_bytes());
        }
        -65536..=-257 => {
            out.push(236);
            out.extend_from_slice(&(v as u16).to_le_bytes());
        }
        -256..=-1 => {
            out.push(235);
            out.push(v as u8);
        }
        0..=231 => out.push(v as u8),
        232..=65535 => {
            out.push(232);
            out.extend_from_slice(&(v as u16).to_le_bytes());
        }
        _ => {
            out.push(233);
            out.extend_from_slice(&(v as u32).to_le_bytes());
        }
    }
}

/// Encodes a non-negative length using the skir variable-length uint32 scheme.
pub(super) fn encode_uint32(n: u32, out: &mut Vec<u8>) {
    match n {
        0..=231 => out.push(n as u8),
        232..=65535 => {
            out.push(232);
            out.extend_from_slice(&(n as u16).to_le_bytes());
        }
        _ => {
            out.push(233);
            out.extend_from_slice(&n.to_le_bytes());
        }
    }
}

// =============================================================================
// BoolAdapter
// =============================================================================

pub(crate) struct BoolAdapter;

impl TypeAdapter<bool> for BoolAdapter {
    fn is_default(&self, input: &bool) -> bool {
        !input
    }

    // Dense mode:    "1" / "0"
    // Readable mode: "true" / "false"
    fn to_json(&self, input: &bool, eol_indent: Option<&str>, out: &mut String) {
        if eol_indent.is_some() {
            out.push_str(if *input { "true" } else { "false" });
        } else {
            out.push(if *input { '1' } else { '0' });
        }
    }

    fn from_json(
        &self,
        json: &serde_json::Value,
        _keep_unrecognized_values: bool,
    ) -> Result<bool, String> {
        match json {
            serde_json::Value::Bool(b) => Ok(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(i != 0)
                } else {
                    Ok(n.as_f64().is_some_and(|f| f != 0.0))
                }
            }
            // Any string other than "0" is truthy; "0" is the only falsy string.
            serde_json::Value::String(s) => Ok(s != "0"),
            _ => Ok(false),
        }
    }

    fn encode(&self, input: &bool, out: &mut Vec<u8>) {
        out.push(u8::from(*input));
    }

    fn decode(
        &self,
        input: &mut &[u8],
        _keep_unrecognized_values: bool,
    ) -> Result<bool, String> {
        match input.first() {
            Some(&b) => {
                *input = &input[1..];
                Ok(b != 0)
            }
            None => Err("unexpected end of input".to_string()),
        }
    }

    fn type_descriptor(&self) -> TypeDescriptor {
        TypeDescriptor::Primitive(PrimitiveType::Bool)
    }

    fn clone_box(&self) -> Box<dyn TypeAdapter<bool>> {
        Box::new(BoolAdapter)
    }
}

// =============================================================================
// Int32Adapter
// =============================================================================

pub(crate) struct Int32Adapter;

impl TypeAdapter<i32> for Int32Adapter {
    fn is_default(&self, input: &i32) -> bool {
        *input == 0
    }

    // Same output in both dense and readable modes — always a JSON number.
    fn to_json(&self, input: &i32, _eol_indent: Option<&str>, out: &mut String) {
        out.push_str(&input.to_string());
    }

    fn from_json(
        &self,
        json: &serde_json::Value,
        _keep_unrecognized_values: bool,
    ) -> Result<i32, String> {
        match json {
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(i as i32)
                } else {
                    Ok(n.as_f64().map(|f| f as i32).unwrap_or(0))
                }
            }
            // Mirrors TypeScript: +(json as string) | 0
            serde_json::Value::String(s) => {
                Ok(s.parse::<f64>().map(|f| f as i32).unwrap_or(0))
            }
            _ => Ok(0),
        }
    }

    fn encode(&self, input: &i32, out: &mut Vec<u8>) {
        encode_i32(*input, out);
    }

    fn decode(
        &self,
        input: &mut &[u8],
        _keep_unrecognized_values: bool,
    ) -> Result<i32, String> {
        decode_number(input).map(|n| n as i32)
    }

    fn type_descriptor(&self) -> TypeDescriptor {
        TypeDescriptor::Primitive(PrimitiveType::Int32)
    }

    fn clone_box(&self) -> Box<dyn TypeAdapter<i32>> {
        Box::new(Int32Adapter)
    }
}

// =============================================================================
// Int64Adapter
// =============================================================================

/// Values within `[-MAX_SAFE_INT, MAX_SAFE_INT]` are emitted as JSON numbers;
/// larger values are quoted strings, matching JS `Number.MAX_SAFE_INTEGER`.
const MAX_SAFE_INT64_JSON: i64 = 9_007_199_254_740_991;

pub(crate) struct Int64Adapter;

impl TypeAdapter<i64> for Int64Adapter {
    fn is_default(&self, input: &i64) -> bool {
        *input == 0
    }

    fn to_json(&self, input: &i64, _eol_indent: Option<&str>, out: &mut String) {
        let v = *input;
        if v >= -MAX_SAFE_INT64_JSON && v <= MAX_SAFE_INT64_JSON {
            out.push_str(&v.to_string());
        } else {
            out.push('"');
            out.push_str(&v.to_string());
            out.push('"');
        }
    }

    fn from_json(
        &self,
        json: &serde_json::Value,
        _keep_unrecognized_values: bool,
    ) -> Result<i64, String> {
        match json {
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(i)
                } else {
                    Ok(n.as_f64().map(|f| f.round() as i64).unwrap_or(0))
                }
            }
            serde_json::Value::String(s) => Ok(s.parse::<i64>().unwrap_or(0)),
            _ => Ok(0),
        }
    }

    fn encode(&self, input: &i64, out: &mut Vec<u8>) {
        let v = *input;
        if v >= i32::MIN as i64 && v <= i32::MAX as i64 {
            encode_i32(v as i32, out);
        } else {
            out.push(238);
            out.extend_from_slice(&v.to_le_bytes());
        }
    }

    fn decode(
        &self,
        input: &mut &[u8],
        _keep_unrecognized_values: bool,
    ) -> Result<i64, String> {
        decode_number(input)
    }

    fn type_descriptor(&self) -> TypeDescriptor {
        TypeDescriptor::Primitive(PrimitiveType::Int64)
    }

    fn clone_box(&self) -> Box<dyn TypeAdapter<i64>> {
        Box::new(Int64Adapter)
    }
}

// =============================================================================
// Hash64Adapter  (uint64 / u64)
// =============================================================================

const MAX_SAFE_HASH64_JSON: u64 = 9_007_199_254_740_991;

pub(crate) struct Hash64Adapter;

impl TypeAdapter<u64> for Hash64Adapter {
    fn is_default(&self, input: &u64) -> bool {
        *input == 0
    }

    fn to_json(&self, input: &u64, _eol_indent: Option<&str>, out: &mut String) {
        let v = *input;
        if v <= MAX_SAFE_HASH64_JSON {
            out.push_str(&v.to_string());
        } else {
            out.push('"');
            out.push_str(&v.to_string());
            out.push('"');
        }
    }

    fn from_json(
        &self,
        json: &serde_json::Value,
        _keep_unrecognized_values: bool,
    ) -> Result<u64, String> {
        match json {
            serde_json::Value::Number(n) => {
                if let Some(u) = n.as_u64() {
                    Ok(u)
                } else {
                    Ok(n.as_f64()
                        .map(|f| if f < 0.0 { 0 } else { f.round() as u64 })
                        .unwrap_or(0))
                }
            }
            serde_json::Value::String(s) => Ok(s.parse::<u64>().unwrap_or(0)),
            _ => Ok(0),
        }
    }

    fn encode(&self, input: &u64, out: &mut Vec<u8>) {
        let v = *input;
        match v {
            0..=231 => out.push(v as u8),
            232..=65535 => {
                out.push(232);
                out.extend_from_slice(&(v as u16).to_le_bytes());
            }
            65536..=4_294_967_295 => {
                out.push(233);
                out.extend_from_slice(&(v as u32).to_le_bytes());
            }
            _ => {
                out.push(234);
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
    }

    fn decode(
        &self,
        input: &mut &[u8],
        _keep_unrecognized_values: bool,
    ) -> Result<u64, String> {
        decode_number(input).map(|n| n as u64)
    }

    fn type_descriptor(&self) -> TypeDescriptor {
        TypeDescriptor::Primitive(PrimitiveType::Hash64)
    }

    fn clone_box(&self) -> Box<dyn TypeAdapter<u64>> {
        Box::new(Hash64Adapter)
    }
}

// =============================================================================
// Float32Adapter
// =============================================================================

/// Returns the TypeScript-compatible string for NaN / ±Infinity.
fn float_special_string(f: f64) -> &'static str {
    if f.is_nan() {
        "NaN"
    } else if f.is_infinite() && f > 0.0 {
        "Infinity"
    } else {
        "-Infinity"
    }
}

pub(crate) struct Float32Adapter;

impl TypeAdapter<f32> for Float32Adapter {
    fn is_default(&self, input: &f32) -> bool {
        *input == 0.0
    }

    // Finite values → shortest round-trip decimal; non-finite → quoted special string.
    fn to_json(&self, input: &f32, _eol_indent: Option<&str>, out: &mut String) {
        let f = *input as f64;
        if f.is_infinite() || f.is_nan() {
            out.push('"');
            out.push_str(float_special_string(f));
            out.push('"');
        } else {
            // `{:?}` gives shortest round-trip repr for f32 cast to f64.
            // Using ryu via format with precision -1 equivalent: just use Rust default.
            out.push_str(&format!("{}", *input));
        }
    }

    fn from_json(
        &self,
        json: &serde_json::Value,
        _keep_unrecognized_values: bool,
    ) -> Result<f32, String> {
        match json {
            serde_json::Value::Number(n) => Ok(n.as_f64().unwrap_or(0.0) as f32),
            serde_json::Value::String(s) => Ok(s.parse::<f32>().unwrap_or(0.0)),
            _ => Ok(0.0),
        }
    }

    // 0 → wire 0; else wire 240 + f32 LE bits.
    fn encode(&self, input: &f32, out: &mut Vec<u8>) {
        if *input == 0.0 {
            out.push(0);
        } else {
            out.push(240);
            out.extend_from_slice(&input.to_bits().to_le_bytes());
        }
    }

    fn decode(
        &self,
        input: &mut &[u8],
        _keep_unrecognized_values: bool,
    ) -> Result<f32, String> {
        let wire = read_u8(input)?;
        if wire == 240 {
            read_f32(input)
        } else {
            decode_number_body(wire, input).map(|n| n as f32)
        }
    }

    fn type_descriptor(&self) -> TypeDescriptor {
        TypeDescriptor::Primitive(PrimitiveType::Float32)
    }

    fn clone_box(&self) -> Box<dyn TypeAdapter<f32>> {
        Box::new(Float32Adapter)
    }
}

// =============================================================================
// Float64Adapter
// =============================================================================

pub(crate) struct Float64Adapter;

impl TypeAdapter<f64> for Float64Adapter {
    fn is_default(&self, input: &f64) -> bool {
        *input == 0.0
    }

    fn to_json(&self, input: &f64, _eol_indent: Option<&str>, out: &mut String) {
        let f = *input;
        if f.is_infinite() || f.is_nan() {
            out.push('"');
            out.push_str(float_special_string(f));
            out.push('"');
        } else {
            out.push_str(&format!("{}", f));
        }
    }

    fn from_json(
        &self,
        json: &serde_json::Value,
        _keep_unrecognized_values: bool,
    ) -> Result<f64, String> {
        match json {
            serde_json::Value::Number(n) => Ok(n.as_f64().unwrap_or(0.0)),
            serde_json::Value::String(s) => Ok(s.parse::<f64>().unwrap_or(0.0)),
            _ => Ok(0.0),
        }
    }

    // 0 → wire 0; else wire 241 + f64 LE bits.
    fn encode(&self, input: &f64, out: &mut Vec<u8>) {
        if *input == 0.0 {
            out.push(0);
        } else {
            out.push(241);
            out.extend_from_slice(&input.to_bits().to_le_bytes());
        }
    }

    fn decode(
        &self,
        input: &mut &[u8],
        _keep_unrecognized_values: bool,
    ) -> Result<f64, String> {
        let wire = read_u8(input)?;
        if wire == 241 {
            read_f64(input)
        } else {
            decode_number_body(wire, input).map(|n| n as f64)
        }
    }

    fn type_descriptor(&self) -> TypeDescriptor {
        TypeDescriptor::Primitive(PrimitiveType::Float64)
    }

    fn clone_box(&self) -> Box<dyn TypeAdapter<f64>> {
        Box::new(Float64Adapter)
    }
}

// =============================================================================
// Timestamp helpers
// =============================================================================

const MIN_TIMESTAMP_MILLIS: i64 = -8_640_000_000_000_000;
const MAX_TIMESTAMP_MILLIS: i64 = 8_640_000_000_000_000;

/// Converts a [`SystemTime`] to unix milliseconds (clamped to valid range).
fn system_time_to_millis(t: SystemTime) -> i64 {
    let ms = match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_millis() as i64,
        Err(e) => -(e.duration().as_millis() as i64),
    };
    ms.clamp(MIN_TIMESTAMP_MILLIS, MAX_TIMESTAMP_MILLIS)
}

/// Creates a [`SystemTime`] from unix milliseconds.
fn millis_to_system_time(ms: i64) -> SystemTime {
    let ms = ms.clamp(MIN_TIMESTAMP_MILLIS, MAX_TIMESTAMP_MILLIS);
    if ms >= 0 {
        SystemTime::UNIX_EPOCH + Duration::from_millis(ms as u64)
    } else {
        SystemTime::UNIX_EPOCH - Duration::from_millis((-ms) as u64)
    }
}

/// Converts a Unix-millisecond value to an ISO-8601 UTC string with millisecond
/// precision, e.g. `"2009-02-13T23:31:30.000Z"`.
///
/// Uses Howard Hinnant's civil-from-days algorithm.
/// <https://howardhinnant.github.io/date_algorithms.html>
fn millis_to_iso8601(ms: i64) -> String {
    let ms = ms.clamp(MIN_TIMESTAMP_MILLIS, MAX_TIMESTAMP_MILLIS);
    let millis_part = ms.rem_euclid(1000) as u32;
    let secs = ms.div_euclid(1000);
    let time_of_day = secs.rem_euclid(86400) as u32;
    let h = time_of_day / 3600;
    let mi = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;

    let days = secs.div_euclid(86400);
    let z = days + 719_468_i64;
    let era = z.div_euclid(146_097_i64);
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y: i64 = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", y, m, d, h, mi, s, millis_part)
}

// =============================================================================
// String helpers
// =============================================================================

/// Writes `s` as a JSON string literal to `out`, escaping `"`, `\`, and
/// control characters.
pub(super) fn write_json_escaped_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0C' => out.push_str("\\f"),
            c if c < '\x20' || c == '\x7F' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

// =============================================================================
// Base64 and hex helpers
// =============================================================================

const BASE64_ALPHABET: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes `bytes` to standard base64 with `=` padding.
fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(BASE64_ALPHABET[(triple >> 18) as usize] as char);
        out.push(BASE64_ALPHABET[((triple >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            BASE64_ALPHABET[((triple >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64_ALPHABET[(triple & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode_char(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decodes a standard base64 string (`=` padding is stripped).
fn decode_base64(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4 + 1);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for &b in s.as_bytes() {
        let v = base64_decode_char(b)
            .ok_or_else(|| format!("invalid base64 character: {:?}", b as char))?;
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Ok(out)
}

/// Encodes `bytes` to a lowercase hexadecimal string.
fn encode_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((b & 0xF) as u32, 16).unwrap());
    }
    out
}

/// Decodes a lowercase or uppercase hexadecimal string.
fn decode_hex(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err(format!("odd hex string length: {}", s.len()));
    }
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

// =============================================================================
// TimestampAdapter
// =============================================================================

pub(crate) struct TimestampAdapter;

impl TypeAdapter<SystemTime> for TimestampAdapter {
    fn is_default(&self, input: &SystemTime) -> bool {
        system_time_to_millis(*input) == 0
    }

    // Dense: unix millis as a JSON number.
    // Readable: {"unix_millis": N, "formatted": "<ISO-8601>"}.
    fn to_json(&self, input: &SystemTime, eol_indent: Option<&str>, out: &mut String) {
        let ms = system_time_to_millis(*input);
        if let Some(eol) = eol_indent {
            let child = format!("{}  ", eol);
            out.push('{');
            out.push_str(&child);
            out.push_str("\"unix_millis\": ");
            out.push_str(&ms.to_string());
            out.push(',');
            out.push_str(&child);
            out.push_str("\"formatted\": \"");
            out.push_str(&millis_to_iso8601(ms));
            out.push('"');
            out.push_str(eol);
            out.push('}');
        } else {
            out.push_str(&ms.to_string());
        }
    }

    fn from_json(
        &self,
        json: &serde_json::Value,
        _keep_unrecognized_values: bool,
    ) -> Result<SystemTime, String> {
        let ms = match json {
            serde_json::Value::Number(n) => n
                .as_i64()
                .unwrap_or_else(|| n.as_f64().map(|f| f.round() as i64).unwrap_or(0)),
            serde_json::Value::String(s) => {
                s.parse::<f64>().map(|f| f.round() as i64).unwrap_or(0)
            }
            // Readable format: {"unix_millis": N, "formatted": "..."}
            serde_json::Value::Object(map) => match map.get("unix_millis") {
                Some(field) => return self.from_json(field, false),
                None => 0,
            },
            _ => 0,
        };
        Ok(millis_to_system_time(ms))
    }

    // millis == 0 → wire 0; else → wire 239 + i64 LE.
    fn encode(&self, input: &SystemTime, out: &mut Vec<u8>) {
        let ms = system_time_to_millis(*input);
        if ms == 0 {
            out.push(0);
        } else {
            out.push(239);
            out.extend_from_slice(&ms.to_le_bytes());
        }
    }

    fn decode(
        &self,
        input: &mut &[u8],
        _keep_unrecognized_values: bool,
    ) -> Result<SystemTime, String> {
        decode_number(input).map(millis_to_system_time)
    }

    fn type_descriptor(&self) -> TypeDescriptor {
        TypeDescriptor::Primitive(PrimitiveType::Timestamp)
    }

    fn clone_box(&self) -> Box<dyn TypeAdapter<SystemTime>> {
        Box::new(TimestampAdapter)
    }
}

// =============================================================================
// StringAdapter
// =============================================================================

pub(crate) struct StringAdapter;

impl TypeAdapter<String> for StringAdapter {
    fn is_default(&self, input: &String) -> bool {
        input.is_empty()
    }

    // Same in both dense and readable modes — always a JSON string.
    fn to_json(&self, input: &String, _eol_indent: Option<&str>, out: &mut String) {
        write_json_escaped_string(input, out);
    }

    fn from_json(
        &self,
        json: &serde_json::Value,
        _keep_unrecognized_values: bool,
    ) -> Result<String, String> {
        match json {
            serde_json::Value::String(s) => Ok(s.clone()),
            // Dense default: any number → empty string.
            serde_json::Value::Number(_) => Ok(String::new()),
            _ => Ok(String::new()),
        }
    }

    // empty → wire 242; else → wire 243 + encode_uint32(len) + utf8 bytes.
    fn encode(&self, input: &String, out: &mut Vec<u8>) {
        if input.is_empty() {
            out.push(242);
        } else {
            out.push(243);
            encode_uint32(input.len() as u32, out);
            out.extend_from_slice(input.as_bytes());
        }
    }

    fn decode(
        &self,
        input: &mut &[u8],
        _keep_unrecognized_values: bool,
    ) -> Result<String, String> {
        let wire = read_u8(input)?;
        if wire == 0 || wire == 242 {
            return Ok(String::new());
        }
        let n = decode_number(input)? as usize;
        if input.len() < n {
            return Err("unexpected end of input".to_string());
        }
        let s = String::from_utf8_lossy(&input[..n]).into_owned();
        *input = &input[n..];
        Ok(s)
    }

    fn type_descriptor(&self) -> TypeDescriptor {
        TypeDescriptor::Primitive(PrimitiveType::String)
    }

    fn clone_box(&self) -> Box<dyn TypeAdapter<String>> {
        Box::new(StringAdapter)
    }
}

// =============================================================================
// BytesAdapter
// =============================================================================

pub(crate) struct BytesAdapter;

impl TypeAdapter<Vec<u8>> for BytesAdapter {
    fn is_default(&self, input: &Vec<u8>) -> bool {
        input.is_empty()
    }

    // Dense: standard base64 with padding.
    // Readable: "hex:" + lowercase hex string.
    fn to_json(&self, input: &Vec<u8>, eol_indent: Option<&str>, out: &mut String) {
        out.push('"');
        if eol_indent.is_some() {
            out.push_str("hex:");
            out.push_str(&encode_hex(input));
        } else {
            out.push_str(&encode_base64(input));
        }
        out.push('"');
    }

    fn from_json(
        &self,
        json: &serde_json::Value,
        _keep_unrecognized_values: bool,
    ) -> Result<Vec<u8>, String> {
        match json {
            // Dense default: any number → empty bytes.
            serde_json::Value::Number(_) => Ok(Vec::new()),
            serde_json::Value::String(s) => {
                if let Some(hex) = s.strip_prefix("hex:") {
                    decode_hex(hex)
                } else {
                    decode_base64(s)
                }
            }
            _ => Ok(Vec::new()),
        }
    }

    // empty → wire 244; else → wire 245 + encode_uint32(len) + raw bytes.
    fn encode(&self, input: &Vec<u8>, out: &mut Vec<u8>) {
        if input.is_empty() {
            out.push(244);
        } else {
            out.push(245);
            encode_uint32(input.len() as u32, out);
            out.extend_from_slice(input);
        }
    }

    fn decode(
        &self,
        input: &mut &[u8],
        _keep_unrecognized_values: bool,
    ) -> Result<Vec<u8>, String> {
        let wire = read_u8(input)?;
        if wire == 0 || wire == 244 {
            return Ok(Vec::new());
        }
        let n = decode_number(input)? as usize;
        if input.len() < n {
            return Err("unexpected end of input".to_string());
        }
        let bytes = input[..n].to_vec();
        *input = &input[n..];
        Ok(bytes)
    }

    fn type_descriptor(&self) -> TypeDescriptor {
        TypeDescriptor::Primitive(PrimitiveType::Bytes)
    }

    fn clone_box(&self) -> Box<dyn TypeAdapter<Vec<u8>>> {
        Box::new(BytesAdapter)
    }
}

// =============================================================================
// ArrayAdapter
// =============================================================================

pub(crate) struct ArrayAdapter<T: 'static> {
    item: Serializer<T>,
}

impl<T: 'static> TypeAdapter<Vec<T>> for ArrayAdapter<T> {
    fn is_default(&self, input: &Vec<T>) -> bool {
        input.is_empty()
    }

    // Dense: [item,item,...] — Readable: with newlines and 2-space indentation.
    fn to_json(&self, input: &Vec<T>, eol_indent: Option<&str>, out: &mut String) {
        out.push('[');
        if let Some(eol) = eol_indent {
            let child_eol = format!("{}  ", eol);
            for (i, item) in input.iter().enumerate() {
                out.push_str(&child_eol);
                self.item.adapter().to_json(item, Some(&child_eol), out);
                if i + 1 < input.len() {
                    out.push(',');
                }
            }
            if !input.is_empty() {
                out.push_str(eol);
            }
        } else {
            for (i, item) in input.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                self.item.adapter().to_json(item, None, out);
            }
        }
        out.push(']');
    }

    fn from_json(
        &self,
        json: &serde_json::Value,
        keep_unrecognized_values: bool,
    ) -> Result<Vec<T>, String> {
        match json {
            serde_json::Value::Array(arr) => arr
                .iter()
                .map(|v| self.item.adapter().from_json(v, keep_unrecognized_values))
                .collect(),
            _ => Ok(vec![]),
        }
    }

    // 0 items → wire 246; 1-3 items → wire 247-249 (no length follows);
    // 4+ items → wire 250 + encode_uint32(count).
    fn encode(&self, input: &Vec<T>, out: &mut Vec<u8>) {
        let n = input.len();
        if n <= 3 {
            out.push(246 + n as u8);
        } else {
            out.push(250);
            encode_uint32(n as u32, out);
        }
        for item in input {
            self.item.adapter().encode(item, out);
        }
    }

    fn decode(
        &self,
        input: &mut &[u8],
        keep_unrecognized_values: bool,
    ) -> Result<Vec<T>, String> {
        let wire = read_u8(input)?;
        if wire == 0 || wire == 246 {
            return Ok(vec![]);
        }
        let n = if wire == 250 {
            decode_number(input)? as usize
        } else {
            (wire - 246) as usize
        };
        let mut items = Vec::with_capacity(n);
        for _ in 0..n {
            items.push(self.item.adapter().decode(input, keep_unrecognized_values)?);
        }
        Ok(items)
    }

    fn type_descriptor(&self) -> TypeDescriptor {
        TypeDescriptor::Array(Box::new(ArrayDescriptor::new(
            self.item.adapter().type_descriptor(),
            String::new(),
        )))
    }

    fn clone_box(&self) -> Box<dyn TypeAdapter<Vec<T>>> {
        Box::new(ArrayAdapter { item: self.item.clone() })
    }
}

// =============================================================================
// KeyedArrayAdapter
// =============================================================================

pub(crate) struct KeyedArrayAdapter<S: KeyedVecSpec + 'static> {
    item: Serializer<S::Item>,
}

impl<S: KeyedVecSpec + 'static> TypeAdapter<KeyedVec<S>> for KeyedArrayAdapter<S> {
    fn is_default(&self, input: &KeyedVec<S>) -> bool {
        input.is_empty()
    }

    // Dense: [item,item,...] — Readable: with newlines and 2-space indentation.
    fn to_json(&self, input: &KeyedVec<S>, eol_indent: Option<&str>, out: &mut String) {
        out.push('[');
        if let Some(eol) = eol_indent {
            let child_eol = format!("{}  ", eol);
            for (i, item) in input.iter().enumerate() {
                out.push_str(&child_eol);
                self.item.adapter().to_json(item, Some(&child_eol), out);
                if i + 1 < input.len() {
                    out.push(',');
                }
            }
            if !input.is_empty() {
                out.push_str(eol);
            }
        } else {
            for (i, item) in input.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                self.item.adapter().to_json(item, None, out);
            }
        }
        out.push(']');
    }

    fn from_json(
        &self,
        json: &serde_json::Value,
        keep_unrecognized_values: bool,
    ) -> Result<KeyedVec<S>, String> {
        match json {
            serde_json::Value::Array(arr) => {
                let items: Result<Vec<S::Item>, String> = arr
                    .iter()
                    .map(|v| self.item.adapter().from_json(v, keep_unrecognized_values))
                    .collect();
                items.map(KeyedVec::new)
            }
            _ => Ok(KeyedVec::default()),
        }
    }

    // 0 items → wire 246; 1-3 items → wire 247-249 (no length follows);
    // 4+ items → wire 250 + encode_uint32(count).
    fn encode(&self, input: &KeyedVec<S>, out: &mut Vec<u8>) {
        let n = input.len();
        if n <= 3 {
            out.push(246 + n as u8);
        } else {
            out.push(250);
            encode_uint32(n as u32, out);
        }
        for item in input.iter() {
            self.item.adapter().encode(item, out);
        }
    }

    fn decode(
        &self,
        input: &mut &[u8],
        keep_unrecognized_values: bool,
    ) -> Result<KeyedVec<S>, String> {
        let wire = read_u8(input)?;
        if wire == 0 || wire == 246 {
            return Ok(KeyedVec::default());
        }
        let n = if wire == 250 {
            decode_number(input)? as usize
        } else {
            (wire - 246) as usize
        };
        let mut items = Vec::with_capacity(n);
        for _ in 0..n {
            items.push(self.item.adapter().decode(input, keep_unrecognized_values)?);
        }
        Ok(KeyedVec::new(items))
    }

    fn type_descriptor(&self) -> TypeDescriptor {
        TypeDescriptor::Array(Box::new(ArrayDescriptor::new(
            self.item.adapter().type_descriptor(),
            S::key_extractor().to_string(),
        )))
    }

    fn clone_box(&self) -> Box<dyn TypeAdapter<KeyedVec<S>>> {
        Box::new(KeyedArrayAdapter { item: self.item.clone() })
    }
}

// =============================================================================
// OptionalAdapter
// =============================================================================

pub(crate) struct OptionalAdapter<T: 'static> {
    other: Serializer<T>,
}

impl<T: 'static> TypeAdapter<Option<T>> for OptionalAdapter<T> {
    fn is_default(&self, input: &Option<T>) -> bool {
        input.is_none()
    }

    // None → JSON `null`; Some(v) → delegate to inner adapter.
    fn to_json(&self, input: &Option<T>, eol_indent: Option<&str>, out: &mut String) {
        match input {
            None => out.push_str("null"),
            Some(v) => self.other.adapter().to_json(v, eol_indent, out),
        }
    }

    fn from_json(
        &self,
        json: &serde_json::Value,
        keep_unrecognized_values: bool,
    ) -> Result<Option<T>, String> {
        if json.is_null() {
            return Ok(None);
        }
        self.other.adapter().from_json(json, keep_unrecognized_values).map(Some)
    }

    // None → wire 255; Some(v) → delegate (inner writes its own wire byte).
    fn encode(&self, input: &Option<T>, out: &mut Vec<u8>) {
        match input {
            None => out.push(255),
            Some(v) => self.other.adapter().encode(v, out),
        }
    }

    // Peek at the next byte: 255 → consume it and return None;
    // otherwise let the inner adapter read normally.
    fn decode(
        &self,
        input: &mut &[u8],
        keep_unrecognized_values: bool,
    ) -> Result<Option<T>, String> {
        if input.first() == Some(&255) {
            *input = &input[1..];
            return Ok(None);
        }
        self.other.adapter().decode(input, keep_unrecognized_values).map(Some)
    }

    fn type_descriptor(&self) -> TypeDescriptor {
        TypeDescriptor::Optional(Box::new(self.other.adapter().type_descriptor()))
    }

    fn clone_box(&self) -> Box<dyn TypeAdapter<Option<T>>> {
        Box::new(OptionalAdapter { other: self.other.clone() })
    }
}

// =============================================================================
// RecursiveAdapter
// =============================================================================

/// Serializer for hard-recursive optional fields (`Option<Box<T>>`-style).
///
/// The encoded "absent" sentinel is `[]` (JSON) / `0x_f6` wire byte (`246`),
/// matching how Skir encodes a default struct.  This differs from
/// [`OptionalAdapter`] which uses JSON `null` / wire `0xff`.
pub(crate) struct RecursiveAdapter<T: 'static> {
    other: Serializer<T>,
}

impl<T: 'static> TypeAdapter<Option<T>> for RecursiveAdapter<T> {
    fn is_default(&self, input: &Option<T>) -> bool {
        match input {
            None => true,
            Some(v) => self.other.adapter().is_default(v),
        }
    }

    fn to_json(&self, input: &Option<T>, eol_indent: Option<&str>, out: &mut String) {
        match input {
            None => out.push_str("[]"),
            Some(v) => self.other.adapter().to_json(v, eol_indent, out),
        }
    }

    fn from_json(
        &self,
        json: &serde_json::Value,
        keep_unrecognized_values: bool,
    ) -> Result<Option<T>, String> {
        // Empty JSON array `[]` or number `0` → absent.
        if let serde_json::Value::Array(arr) = json {
            if arr.is_empty() {
                return Ok(None);
            }
        }
        if json == &serde_json::Value::Number(serde_json::Number::from(0)) {
            return Ok(None);
        }
        self.other.adapter().from_json(json, keep_unrecognized_values).map(Some)
    }

    // None → wire 246 (= empty struct/array); Some(v) → delegate.
    fn encode(&self, input: &Option<T>, out: &mut Vec<u8>) {
        match input {
            None => out.push(246),
            Some(v) => self.other.adapter().encode(v, out),
        }
    }

    // Wire 246 or 0 → absent; otherwise delegate.
    fn decode(
        &self,
        input: &mut &[u8],
        keep_unrecognized_values: bool,
    ) -> Result<Option<T>, String> {
        match input.first() {
            Some(&246) | Some(&0) => {
                *input = &input[1..];
                Ok(None)
            }
            _ => self.other.adapter().decode(input, keep_unrecognized_values).map(Some),
        }
    }

    fn type_descriptor(&self) -> TypeDescriptor {
        self.other.adapter().type_descriptor()
    }

    fn clone_box(&self) -> Box<dyn TypeAdapter<Option<T>>> {
        Box::new(RecursiveAdapter { other: self.other.clone() })
    }
}

/// Advances `input` past one complete encoded value without decoding it.
/// Used for removed fields/variants and for unrecognized data from a newer schema.
pub(super) fn skip_value(input: &mut &[u8]) -> Result<(), String> {
    let wire = read_u8(input)?;
    match wire {
        0..=231 => {
            // Single-byte value; already consumed.
        }
        232 | 236 => {
            // 2-byte payload
            if input.len() < 2 {
                return Err("unexpected end of input in skip_value".to_string());
            }
            *input = &input[2..];
        }
        233 | 237 | 240 => {
            // 4-byte payload
            if input.len() < 4 {
                return Err("unexpected end of input in skip_value".to_string());
            }
            *input = &input[4..];
        }
        234 | 238 | 239 | 241 => {
            // 8-byte payload
            if input.len() < 8 {
                return Err("unexpected end of input in skip_value".to_string());
            }
            *input = &input[8..];
        }
        235 => {
            // 1-byte payload
            if input.is_empty() {
                return Err("unexpected end of input in skip_value".to_string());
            }
            *input = &input[1..];
        }
        242 | 244 | 246 => {
            // Empty string, empty bytes, or empty array/struct: nothing further.
        }
        243 | 245 => {
            // String or bytes with length prefix followed by N bytes.
            let n = decode_number(input)? as usize;
            if input.len() < n {
                return Err("unexpected end of input in skip_value".to_string());
            }
            *input = &input[n..];
        }
        247 | 248 | 249 => {
            let n = (wire as usize) - 246;
            for _ in 0..n {
                skip_value(input)?;
            }
        }
        250 => {
            let n = decode_number(input)? as usize;
            for _ in 0..n {
                skip_value(input)?;
            }
        }
        251..=254 => {
            // Enum wrapper variant with small number (250+N, N=1..=4): skip the
            // one wrapped value.
            skip_value(input)?;
        }
        255 => {
            // Optional absent: nothing further.
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── to_json ───────────────────────────────────────────────────────────────

    #[test]
    fn to_json_dense_true() {
        assert_eq!(bool_serializer().to_json(&true, false), "1");
    }

    #[test]
    fn to_json_dense_false() {
        assert_eq!(bool_serializer().to_json(&false, false), "0");
    }

    #[test]
    fn to_json_readable_true() {
        assert_eq!(bool_serializer().to_json(&true, true), "true");
    }

    #[test]
    fn to_json_readable_false() {
        assert_eq!(bool_serializer().to_json(&false, true), "false");
    }

    // ── from_json ─────────────────────────────────────────────────────────────

    #[test]
    fn from_json_bool_literal() {
        let s = bool_serializer();
        assert_eq!(s.from_json("true", false).unwrap(), true);
        assert_eq!(s.from_json("false", false).unwrap(), false);
    }

    #[test]
    fn from_json_number_1_and_0() {
        let s = bool_serializer();
        assert_eq!(s.from_json("1", false).unwrap(), true);
        assert_eq!(s.from_json("0", false).unwrap(), false);
    }

    #[test]
    fn from_json_number_nonzero() {
        assert_eq!(bool_serializer().from_json("42", false).unwrap(), true);
    }

    #[test]
    fn from_json_float_zero() {
        assert_eq!(bool_serializer().from_json("0.0", false).unwrap(), false);
    }

    #[test]
    fn from_json_string_zero_is_false() {
        // The string "0" is the only falsy string value.
        assert_eq!(bool_serializer().from_json(r#""0""#, false).unwrap(), false);
    }

    #[test]
    fn from_json_string_nonzero_is_true() {
        assert_eq!(bool_serializer().from_json(r#""1""#, false).unwrap(), true);
        assert_eq!(bool_serializer().from_json(r#""true""#, false).unwrap(), true);
    }

    #[test]
    fn from_json_null_is_false() {
        assert_eq!(bool_serializer().from_json("null", false).unwrap(), false);
    }

    // ── binary round-trip ─────────────────────────────────────────────────────

    #[test]
    fn binary_round_trip_true() {
        let s = bool_serializer();
        let bytes = s.to_bytes(&true);
        assert_eq!(s.from_bytes(&bytes, false).unwrap(), true);
    }

    #[test]
    fn binary_round_trip_false() {
        let s = bool_serializer();
        let bytes = s.to_bytes(&false);
        assert_eq!(s.from_bytes(&bytes, false).unwrap(), false);
    }

    #[test]
    fn binary_encoding_true_is_skir_then_1() {
        assert_eq!(bool_serializer().to_bytes(&true), b"skir\x01");
    }

    #[test]
    fn binary_encoding_false_is_skir_then_0() {
        assert_eq!(bool_serializer().to_bytes(&false), b"skir\x00");
    }

    // ── type_descriptor ───────────────────────────────────────────────────────

    #[test]
    fn type_descriptor_is_bool() {
        let td = bool_serializer().type_descriptor();
        assert_eq!(
            td.as_json(),
            "{\n  \"type\": {\n    \"kind\": \"primitive\",\n    \"value\": \"bool\"\n  },\n  \"records\": []\n}"
        );
    }

    // ── clone ─────────────────────────────────────────────────────────────────

    #[test]
    fn clone_produces_equivalent_serializer() {
        let s = bool_serializer();
        let s2 = s.clone();
        assert_eq!(s2.to_json(&true, false), "1");
        assert_eq!(s2.to_json(&false, true), "false");
    }

    // =========================================================================
    // int32_serializer
    // =========================================================================

    // ── to_json ───────────────────────────────────────────────────────────────

    #[test]
    fn int32_to_json_zero() {
        assert_eq!(int32_serializer().to_json(&0_i32, false), "0");
    }

    #[test]
    fn int32_to_json_positive() {
        assert_eq!(int32_serializer().to_json(&42_i32, false), "42");
    }

    #[test]
    fn int32_to_json_negative() {
        assert_eq!(int32_serializer().to_json(&-1_i32, false), "-1");
    }

    #[test]
    fn int32_to_json_same_in_readable_mode() {
        assert_eq!(
            int32_serializer().to_json(&12345_i32, true),
            int32_serializer().to_json(&12345_i32, false),
        );
    }

    // ── from_json ─────────────────────────────────────────────────────────────

    #[test]
    fn int32_from_json_integer() {
        let s = int32_serializer();
        assert_eq!(s.from_json("42", false).unwrap(), 42_i32);
        assert_eq!(s.from_json("-1", false).unwrap(), -1_i32);
        assert_eq!(s.from_json("0", false).unwrap(), 0_i32);
    }

    #[test]
    fn int32_from_json_float_truncates() {
        assert_eq!(int32_serializer().from_json("3.9", false).unwrap(), 3_i32);
        assert_eq!(int32_serializer().from_json("-1.5", false).unwrap(), -1_i32);
    }

    #[test]
    fn int32_from_json_string() {
        assert_eq!(int32_serializer().from_json(r#""7""#, false).unwrap(), 7_i32);
    }

    #[test]
    fn int32_from_json_unparseable_string_is_zero() {
        assert_eq!(int32_serializer().from_json(r#""abc""#, false).unwrap(), 0_i32);
    }

    #[test]
    fn int32_from_json_null_is_zero() {
        assert_eq!(int32_serializer().from_json("null", false).unwrap(), 0_i32);
    }

    // ── binary encoding ───────────────────────────────────────────────────────

    #[test]
    fn int32_encode_small_positive_is_single_byte() {
        // 0..=231 encoded as the value itself
        let s = int32_serializer();
        assert_eq!(s.to_bytes(&0_i32), b"skir\x00");
        assert_eq!(s.to_bytes(&1_i32), b"skir\x01");
        assert_eq!(s.to_bytes(&231_i32), b"skir\xe7");
    }

    #[test]
    fn int32_encode_u16_range() {
        // 232..65535 → wire 232 + u16 LE
        let bytes = int32_serializer().to_bytes(&1000_i32);
        assert_eq!(&bytes[4..], &[232, 232, 3]); // 1000 = 0x03E8 LE
    }

    #[test]
    fn int32_encode_u32_range() {
        // >= 65536 → wire 233 + u32 LE
        let bytes = int32_serializer().to_bytes(&65536_i32);
        assert_eq!(&bytes[4..], &[233, 0, 0, 1, 0]);
    }

    #[test]
    fn int32_encode_small_negative() {
        // -256..-1 → wire 235 + u8(v+256)
        let bytes = int32_serializer().to_bytes(&-1_i32);
        assert_eq!(&bytes[4..], &[235, 255]);
    }

    #[test]
    fn int32_encode_medium_negative() {
        // -65536..-257 → wire 236 + u16 LE
        let bytes = int32_serializer().to_bytes(&-300_i32);
        assert_eq!(&bytes[4..], &[236, 212, 254]); // -300+65536=65236=0xFED4 LE
    }

    #[test]
    fn int32_encode_large_negative() {
        // < -65536 → wire 237 + i32 LE
        let bytes = int32_serializer().to_bytes(&-100_000_i32);
        assert_eq!(&bytes[4..], &[237, 96, 121, 254, 255]); // -100000 as i32 LE
    }

    #[test]
    fn int32_binary_round_trip() {
        let s = int32_serializer();
        for v in [0, 1, 42, 231, 232, 300, 65535, 65536, i32::MAX, -1, -255, -256, -65536, i32::MIN] {
            let decoded = s.from_bytes(&s.to_bytes(&v), false).unwrap();
            assert_eq!(decoded, v, "round trip failed for {v}");
        }
    }

    // =========================================================================
    // int64_serializer
    // =========================================================================

    // ── to_json ───────────────────────────────────────────────────────────────

    #[test]
    fn int64_to_json_safe_integer() {
        assert_eq!(int64_serializer().to_json(&0_i64, false), "0");
        assert_eq!(int64_serializer().to_json(&9_007_199_254_740_991_i64, false), "9007199254740991");
        assert_eq!(int64_serializer().to_json(&-9_007_199_254_740_991_i64, false), "-9007199254740991");
    }

    #[test]
    fn int64_to_json_large_value_is_quoted() {
        assert_eq!(
            int64_serializer().to_json(&9_007_199_254_740_992_i64, false),
            r#""9007199254740992""#,
        );
        assert_eq!(
            int64_serializer().to_json(&i64::MAX, false),
            format!("\"{}\"", i64::MAX),
        );
    }

    // ── from_json ─────────────────────────────────────────────────────────────

    #[test]
    fn int64_from_json_integer() {
        assert_eq!(int64_serializer().from_json("42", false).unwrap(), 42_i64);
        assert_eq!(int64_serializer().from_json("-1", false).unwrap(), -1_i64);
    }

    #[test]
    fn int64_from_json_quoted_large() {
        assert_eq!(
            int64_serializer().from_json(r#""9007199254740992""#, false).unwrap(),
            9_007_199_254_740_992_i64,
        );
    }

    #[test]
    fn int64_from_json_null_is_zero() {
        assert_eq!(int64_serializer().from_json("null", false).unwrap(), 0_i64);
    }

    // ── binary encoding ───────────────────────────────────────────────────────

    #[test]
    fn int64_encode_fits_i32_reuses_i32_encoding() {
        // Values in i32 range reuse int32 wire format.
        assert_eq!(int64_serializer().to_bytes(&0_i64), b"skir\x00");
        assert_eq!(int64_serializer().to_bytes(&42_i64), b"skir\x2a");
    }

    #[test]
    fn int64_encode_wire_238() {
        // Values outside i32 range → wire 238 + i64 LE
        let v: i64 = i32::MAX as i64 + 1;
        let bytes = int64_serializer().to_bytes(&v);
        assert_eq!(bytes[4], 238);
        assert_eq!(&bytes[5..], &v.to_le_bytes());
    }

    #[test]
    fn int64_binary_round_trip() {
        let s = int64_serializer();
        for v in [0, 1, 231, 232, 65536, i32::MAX as i64, i32::MAX as i64 + 1, i64::MAX, -1, i32::MIN as i64, i64::MIN] {
            let decoded = s.from_bytes(&s.to_bytes(&v), false).unwrap();
            assert_eq!(decoded, v, "round trip failed for {v}");
        }
    }

    // =========================================================================
    // uint64_serializer
    // =========================================================================

    // ── to_json ───────────────────────────────────────────────────────────────

    #[test]
    fn uint64_to_json_safe_integer() {
        assert_eq!(hash64_serializer().to_json(&0_u64, false), "0");
        assert_eq!(hash64_serializer().to_json(&9_007_199_254_740_991_u64, false), "9007199254740991");
    }

    #[test]
    fn uint64_to_json_large_value_is_quoted() {
        assert_eq!(
            hash64_serializer().to_json(&9_007_199_254_740_992_u64, false),
            r#""9007199254740992""#,
        );
        assert_eq!(
            hash64_serializer().to_json(&u64::MAX, false),
            format!("\"{}\"", u64::MAX),
        );
    }

    // ── from_json ─────────────────────────────────────────────────────────────

    #[test]
    fn uint64_from_json_integer() {
        assert_eq!(hash64_serializer().from_json("42", false).unwrap(), 42_u64);
    }

    #[test]
    fn uint64_from_json_negative_number_is_zero() {
        // negative float → clamped to 0
        assert_eq!(hash64_serializer().from_json("-1.0", false).unwrap(), 0_u64);
    }

    #[test]
    fn uint64_from_json_quoted_large() {
        assert_eq!(
            hash64_serializer().from_json(r#""9007199254740992""#, false).unwrap(),
            9_007_199_254_740_992_u64,
        );
    }

    #[test]
    fn uint64_from_json_null_is_zero() {
        assert_eq!(hash64_serializer().from_json("null", false).unwrap(), 0_u64);
    }

    // ── binary encoding ───────────────────────────────────────────────────────

    #[test]
    fn uint64_encode_single_byte_range() {
        assert_eq!(hash64_serializer().to_bytes(&0_u64), b"skir\x00");
        assert_eq!(hash64_serializer().to_bytes(&231_u64), b"skir\xe7");
    }

    #[test]
    fn uint64_encode_u16_range() {
        // 232..65535 → wire 232 + u16 LE
        let bytes = hash64_serializer().to_bytes(&1000_u64);
        assert_eq!(&bytes[4..], &[232, 232, 3]);
    }

    #[test]
    fn uint64_encode_u32_range() {
        // 65536..4294967295 → wire 233 + u32 LE
        let bytes = hash64_serializer().to_bytes(&65536_u64);
        assert_eq!(&bytes[4..], &[233, 0, 0, 1, 0]);
    }

    #[test]
    fn uint64_encode_u64_range() {
        // >= 2^32 → wire 234 + u64 LE
        let v: u64 = 4_294_967_296;
        let bytes = hash64_serializer().to_bytes(&v);
        assert_eq!(bytes[4], 234);
        assert_eq!(&bytes[5..], &v.to_le_bytes());
    }

    #[test]
    fn uint64_binary_round_trip() {
        let s = hash64_serializer();
        for v in [0_u64, 1, 231, 232, 65535, 65536, 4_294_967_295, 4_294_967_296, u64::MAX] {
            let decoded = s.from_bytes(&s.to_bytes(&v), false).unwrap();
            assert_eq!(decoded, v, "round trip failed for {v}");
        }
    }

    // =========================================================================
    // float32_serializer
    // =========================================================================

    // ── to_json ───────────────────────────────────────────────────────────────

    #[test]
    fn float32_to_json_zero() {
        assert_eq!(float32_serializer().to_json(&0.0_f32, false), "0");
    }

    #[test]
    fn float32_to_json_finite() {
        assert_eq!(float32_serializer().to_json(&1.5_f32, false), "1.5");
        assert_eq!(float32_serializer().to_json(&-3.14_f32, false), "-3.14");
    }

    #[test]
    fn float32_to_json_nan_is_quoted() {
        assert_eq!(float32_serializer().to_json(&f32::NAN, false), r#""NaN""#);
    }

    #[test]
    fn float32_to_json_infinity_is_quoted() {
        assert_eq!(float32_serializer().to_json(&f32::INFINITY, false), r#""Infinity""#);
        assert_eq!(float32_serializer().to_json(&f32::NEG_INFINITY, false), r#""-Infinity""#);
    }

    #[test]
    fn float32_to_json_same_in_readable_mode() {
        assert_eq!(
            float32_serializer().to_json(&1.5_f32, true),
            float32_serializer().to_json(&1.5_f32, false),
        );
    }

    // ── from_json ─────────────────────────────────────────────────────────────

    #[test]
    fn float32_from_json_number() {
        let v = float32_serializer().from_json("1.5", false).unwrap();
        assert!((v - 1.5_f32).abs() < f32::EPSILON);
    }

    #[test]
    fn float32_from_json_string_nan() {
        assert!(float32_serializer().from_json(r#""NaN""#, false).unwrap().is_nan());
    }

    #[test]
    fn float32_from_json_string_infinity() {
        assert_eq!(float32_serializer().from_json(r#""Infinity""#, false).unwrap(), f32::INFINITY);
        assert_eq!(float32_serializer().from_json(r#""-Infinity""#, false).unwrap(), f32::NEG_INFINITY);
    }

    #[test]
    fn float32_from_json_unparseable_string_is_zero() {
        assert_eq!(float32_serializer().from_json(r#""abc""#, false).unwrap(), 0.0_f32);
    }

    #[test]
    fn float32_from_json_null_is_zero() {
        assert_eq!(float32_serializer().from_json("null", false).unwrap(), 0.0_f32);
    }

    // ── binary encoding ───────────────────────────────────────────────────────

    #[test]
    fn float32_encode_zero_is_single_byte() {
        assert_eq!(float32_serializer().to_bytes(&0.0_f32), b"skir\x00");
    }

    #[test]
    fn float32_encode_nonzero_is_wire_240_plus_le_bits() {
        let v = 1.5_f32;
        let bytes = float32_serializer().to_bytes(&v);
        assert_eq!(bytes[4], 240);
        assert_eq!(&bytes[5..], &v.to_bits().to_le_bytes());
    }

    #[test]
    fn float32_binary_round_trip() {
        let s = float32_serializer();
        for v in [0.0_f32, 1.0, -1.0, 1.5, f32::MAX, f32::MIN_POSITIVE, f32::INFINITY, f32::NEG_INFINITY] {
            let decoded = s.from_bytes(&s.to_bytes(&v), false).unwrap();
            assert_eq!(decoded, v, "round trip failed for {v}");
        }
    }

    #[test]
    fn float32_nan_round_trip() {
        let s = float32_serializer();
        let decoded = s.from_bytes(&s.to_bytes(&f32::NAN), false).unwrap();
        assert!(decoded.is_nan());
    }

    // =========================================================================
    // float64_serializer
    // =========================================================================

    // ── to_json ───────────────────────────────────────────────────────────────

    #[test]
    fn float64_to_json_zero() {
        assert_eq!(float64_serializer().to_json(&0.0_f64, false), "0");
    }

    #[test]
    fn float64_to_json_finite() {
        assert_eq!(float64_serializer().to_json(&1.5_f64, false), "1.5");
        assert_eq!(float64_serializer().to_json(&-3.14_f64, false), "-3.14");
    }

    #[test]
    fn float64_to_json_nan_is_quoted() {
        assert_eq!(float64_serializer().to_json(&f64::NAN, false), r#""NaN""#);
    }

    #[test]
    fn float64_to_json_infinity_is_quoted() {
        assert_eq!(float64_serializer().to_json(&f64::INFINITY, false), r#""Infinity""#);
        assert_eq!(float64_serializer().to_json(&f64::NEG_INFINITY, false), r#""-Infinity""#);
    }

    // ── from_json ─────────────────────────────────────────────────────────────

    #[test]
    fn float64_from_json_number() {
        let v = float64_serializer().from_json("1.5", false).unwrap();
        assert!((v - 1.5_f64).abs() < f64::EPSILON);
    }

    #[test]
    fn float64_from_json_string_nan() {
        assert!(float64_serializer().from_json(r#""NaN""#, false).unwrap().is_nan());
    }

    #[test]
    fn float64_from_json_string_infinity() {
        assert_eq!(float64_serializer().from_json(r#""Infinity""#, false).unwrap(), f64::INFINITY);
        assert_eq!(float64_serializer().from_json(r#""-Infinity""#, false).unwrap(), f64::NEG_INFINITY);
    }

    #[test]
    fn float64_from_json_null_is_zero() {
        assert_eq!(float64_serializer().from_json("null", false).unwrap(), 0.0_f64);
    }

    // ── binary encoding ───────────────────────────────────────────────────────

    #[test]
    fn float64_encode_zero_is_single_byte() {
        assert_eq!(float64_serializer().to_bytes(&0.0_f64), b"skir\x00");
    }

    #[test]
    fn float64_encode_nonzero_is_wire_241_plus_le_bits() {
        let v = 1.5_f64;
        let bytes = float64_serializer().to_bytes(&v);
        assert_eq!(bytes[4], 241);
        assert_eq!(&bytes[5..], &v.to_bits().to_le_bytes());
    }

    #[test]
    fn float64_binary_round_trip() {
        let s = float64_serializer();
        for v in [0.0_f64, 1.0, -1.0, 1.5, f64::MAX, f64::MIN_POSITIVE, f64::INFINITY, f64::NEG_INFINITY] {
            let decoded = s.from_bytes(&s.to_bytes(&v), false).unwrap();
            assert_eq!(decoded, v, "round trip failed for {v}");
        }
    }

    #[test]
    fn float64_nan_round_trip() {
        let s = float64_serializer();
        let decoded = s.from_bytes(&s.to_bytes(&f64::NAN), false).unwrap();
        assert!(decoded.is_nan());
    }

    // =========================================================================
    // timestamp_serializer
    // =========================================================================

    // ── millis_to_iso8601 ─────────────────────────────────────────────────────

    #[test]
    fn millis_to_iso8601_epoch() {
        assert_eq!(millis_to_iso8601(0), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn millis_to_iso8601_known_date() {
        // 2009-02-13T23:31:30.000Z
        assert_eq!(millis_to_iso8601(1_234_567_890_000), "2009-02-13T23:31:30.000Z");
    }

    #[test]
    fn millis_to_iso8601_milliseconds() {
        assert_eq!(millis_to_iso8601(1_234_567_890_123), "2009-02-13T23:31:30.123Z");
    }

    #[test]
    fn millis_to_iso8601_negative() {
        // -1000 ms = 1969-12-31T23:59:59.000Z
        assert_eq!(millis_to_iso8601(-1000), "1969-12-31T23:59:59.000Z");
    }

    // ── to_json ───────────────────────────────────────────────────────────────

    #[test]
    fn timestamp_to_json_dense_epoch() {
        assert_eq!(timestamp_serializer().to_json(&SystemTime::UNIX_EPOCH, false), "0");
    }

    #[test]
    fn timestamp_to_json_dense_nonzero() {
        let ts = millis_to_system_time(1_234_567_890_000);
        assert_eq!(
            timestamp_serializer().to_json(&ts, false),
            "1234567890000",
        );
    }

    #[test]
    fn timestamp_to_json_readable() {
        let ts = millis_to_system_time(1_234_567_890_000);
        let expected =
            "{\n  \"unix_millis\": 1234567890000,\n  \"formatted\": \"2009-02-13T23:31:30.000Z\"\n}";
        assert_eq!(timestamp_serializer().to_json(&ts, true), expected);
    }

    // ── from_json ─────────────────────────────────────────────────────────────

    #[test]
    fn timestamp_from_json_number() {
        assert_eq!(
            timestamp_serializer().from_json("1234567890000", false).unwrap(),
            millis_to_system_time(1_234_567_890_000),
        );
    }

    #[test]
    fn timestamp_from_json_string() {
        assert_eq!(
            timestamp_serializer().from_json(r#""1234567890000""#, false).unwrap(),
            millis_to_system_time(1_234_567_890_000),
        );
    }

    #[test]
    fn timestamp_from_json_object_readable() {
        let json =
            r#"{"unix_millis": 1234567890000, "formatted": "2009-02-13T23:31:30.000Z"}"#;
        assert_eq!(
            timestamp_serializer().from_json(json, false).unwrap(),
            millis_to_system_time(1_234_567_890_000),
        );
    }

    #[test]
    fn timestamp_from_json_null_is_epoch() {
        assert_eq!(
            timestamp_serializer().from_json("null", false).unwrap(),
            SystemTime::UNIX_EPOCH,
        );
    }

    // ── binary encoding ───────────────────────────────────────────────────────

    #[test]
    fn timestamp_encode_epoch_is_single_byte_zero() {
        assert_eq!(timestamp_serializer().to_bytes(&SystemTime::UNIX_EPOCH), b"skir\x00");
    }

    #[test]
    fn timestamp_encode_nonzero_is_wire_239() {
        let ts = millis_to_system_time(1_234_567_890_000);
        let bytes = timestamp_serializer().to_bytes(&ts);
        assert_eq!(bytes[4], 239);
        assert_eq!(&bytes[5..], &1_234_567_890_000_i64.to_le_bytes());
    }

    #[test]
    fn timestamp_binary_round_trip() {
        let s = timestamp_serializer();
        for ms in [0_i64, 1, 1_234_567_890_000, -1000] {
            let ts = millis_to_system_time(ms);
            assert_eq!(s.from_bytes(&s.to_bytes(&ts), false).unwrap(), ts);
        }
    }

    // =========================================================================
    // string_serializer
    // =========================================================================

    // ── to_json ───────────────────────────────────────────────────────────────

    #[test]
    fn string_to_json_plain() {
        assert_eq!(string_serializer().to_json(&"hello".to_string(), false), r#""hello""#);
    }

    #[test]
    fn string_to_json_empty() {
        assert_eq!(string_serializer().to_json(&String::new(), false), r#""""#);
    }

    #[test]
    fn string_to_json_escapes_quote_and_backslash() {
        // Input: say "hi"  →  JSON: "say \"hi\""
        assert_eq!(
            string_serializer().to_json(&"say \"hi\"".to_string(), false),
            "\"say \\\"hi\\\"\""
        );
        // Input: a\b  →  JSON: "a\\b"
        assert_eq!(
            string_serializer().to_json(&"a\\b".to_string(), false),
            "\"a\\\\b\""
        );
    }

    #[test]
    fn string_to_json_escapes_control_chars() {
        // \n, \t, \r should appear as two-char sequences in JSON output.
        assert_eq!(
            string_serializer().to_json(&"\n\t\r".to_string(), false),
            "\"\\n\\t\\r\""
        );
    }

    #[test]
    fn string_to_json_same_in_readable_mode() {
        assert_eq!(
            string_serializer().to_json(&"hello".to_string(), true),
            string_serializer().to_json(&"hello".to_string(), false),
        );
    }

    // ── from_json ─────────────────────────────────────────────────────────────

    #[test]
    fn string_from_json_string() {
        assert_eq!(
            string_serializer().from_json(r#""hello""#, false).unwrap(),
            "hello".to_string(),
        );
    }

    #[test]
    fn string_from_json_number_is_empty() {
        assert_eq!(string_serializer().from_json("0", false).unwrap(), "");
    }

    #[test]
    fn string_from_json_null_is_empty() {
        assert_eq!(string_serializer().from_json("null", false).unwrap(), "");
    }

    // ── binary encoding ───────────────────────────────────────────────────────

    #[test]
    fn string_encode_empty_is_wire_242() {
        assert_eq!(string_serializer().to_bytes(&String::new()), b"skir\xf2");
    }

    #[test]
    fn string_encode_nonempty() {
        // wire 243 + length (1 byte for len < 232) + utf-8 bytes
        let bytes = string_serializer().to_bytes(&"hi".to_string());
        assert_eq!(&bytes[4..], &[0xf3, 0x02, b'h', b'i']);
    }

    #[test]
    fn string_binary_round_trip() {
        let s = string_serializer();
        for v in ["", "hello", "emoji: \u{1F600}", "quotes: \"x\""] {
            let v = v.to_string();
            assert_eq!(s.from_bytes(&s.to_bytes(&v), false).unwrap(), v);
        }
    }

    // =========================================================================
    // bytes_serializer
    // =========================================================================

    // ── base64 helpers ────────────────────────────────────────────────────────

    #[test]
    fn base64_encode_hello() {
        assert_eq!(encode_base64(b"hello"), "aGVsbG8=");
    }

    #[test]
    fn base64_encode_empty() {
        assert_eq!(encode_base64(b""), "");
    }

    #[test]
    fn base64_decode_hello() {
        assert_eq!(decode_base64("aGVsbG8=").unwrap(), b"hello");
    }

    #[test]
    fn base64_round_trip() {
        for data in [b"".as_slice(), b"a", b"ab", b"abc", b"hello world"] {
            assert_eq!(decode_base64(&encode_base64(data)).unwrap(), data);
        }
    }

    // ── hex helpers ───────────────────────────────────────────────────────────

    #[test]
    fn hex_encode_hello() {
        assert_eq!(encode_hex(b"hello"), "68656c6c6f");
    }

    #[test]
    fn hex_round_trip() {
        for data in [b"".as_slice(), b"\x00\xff", b"hello"] {
            assert_eq!(decode_hex(&encode_hex(data)).unwrap(), data);
        }
    }

    // ── to_json ───────────────────────────────────────────────────────────────

    #[test]
    fn bytes_to_json_dense_base64() {
        assert_eq!(
            bytes_serializer().to_json(&b"hello".to_vec(), false),
            r#""aGVsbG8=""#,
        );
    }

    #[test]
    fn bytes_to_json_readable_hex() {
        assert_eq!(
            bytes_serializer().to_json(&b"hello".to_vec(), true),
            r#""hex:68656c6c6f""#,
        );
    }

    #[test]
    fn bytes_to_json_empty_dense() {
        assert_eq!(bytes_serializer().to_json(&vec![], false), r#""""#);
    }

    // ── from_json ─────────────────────────────────────────────────────────────

    #[test]
    fn bytes_from_json_base64() {
        assert_eq!(
            bytes_serializer().from_json(r#""aGVsbG8=""#, false).unwrap(),
            b"hello".to_vec(),
        );
    }

    #[test]
    fn bytes_from_json_hex() {
        assert_eq!(
            bytes_serializer().from_json(r#""hex:68656c6c6f""#, false).unwrap(),
            b"hello".to_vec(),
        );
    }

    #[test]
    fn bytes_from_json_number_is_empty() {
        assert_eq!(bytes_serializer().from_json("0", false).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn bytes_from_json_null_is_empty() {
        assert_eq!(bytes_serializer().from_json("null", false).unwrap(), Vec::<u8>::new());
    }

    // ── binary encoding ───────────────────────────────────────────────────────

    #[test]
    fn bytes_encode_empty_is_wire_244() {
        assert_eq!(bytes_serializer().to_bytes(&vec![]), b"skir\xf4");
    }

    #[test]
    fn bytes_encode_nonempty() {
        // wire 245 + length (1 byte for len < 232) + raw bytes
        let bytes = bytes_serializer().to_bytes(&vec![1_u8, 2, 3]);
        assert_eq!(&bytes[4..], &[0xf5, 0x03, 1, 2, 3]);
    }

    #[test]
    fn bytes_binary_round_trip() {
        let s = bytes_serializer();
        for data in [vec![], vec![0_u8], b"hello".to_vec(), vec![0xFF_u8; 300]] {
            assert_eq!(s.from_bytes(&s.to_bytes(&data), false).unwrap(), data);
        }
    }

    // ── array_serializer ──────────────────────────────────────────────────────

    #[test]
    fn array_to_json_dense_empty() {
        assert_eq!(array_serializer(int32_serializer()).to_json(&vec![], false), "[]");
    }

    #[test]
    fn array_to_json_dense_nonempty() {
        assert_eq!(
            array_serializer(int32_serializer()).to_json(&vec![1_i32, 2, 3], false),
            "[1,2,3]",
        );
    }

    #[test]
    fn array_to_json_readable_empty() {
        assert_eq!(array_serializer(int32_serializer()).to_json(&vec![], true), "[]");
    }

    #[test]
    fn array_to_json_readable_nonempty() {
        assert_eq!(
            array_serializer(int32_serializer()).to_json(&vec![1_i32, 2], true),
            "[\n  1,\n  2\n]",
        );
    }

    #[test]
    fn array_from_json_array() {
        assert_eq!(
            array_serializer(int32_serializer()).from_json("[10,20,30]", false).unwrap(),
            vec![10_i32, 20, 30],
        );
    }

    #[test]
    fn array_from_json_null_is_empty() {
        assert_eq!(
            array_serializer(int32_serializer()).from_json("null", false).unwrap(),
            Vec::<i32>::new(),
        );
    }

    #[test]
    fn array_encode_empty_is_wire_246() {
        assert_eq!(
            array_serializer(int32_serializer()).to_bytes(&vec![]),
            b"skir\xf6",
        );
    }

    #[test]
    fn array_encode_nonempty() {
        // wire 247 + count 3 + items [1, 2, 3] (each a single-byte int32)
        let bytes = array_serializer(int32_serializer()).to_bytes(&vec![1_i32, 2, 3]);
        assert_eq!(&bytes[4..], &[0xf7_u8, 3, 1, 2, 3]);
    }

    #[test]
    fn array_binary_round_trip() {
        let s = array_serializer(int32_serializer());
        for v in [vec![], vec![0_i32], vec![1, 2, 3], vec![-1, 0, 1]] {
            assert_eq!(s.from_bytes(&s.to_bytes(&v), false).unwrap(), v);
        }
    }

    // ── keyed_array_serializer ────────────────────────────────────────────────

    struct I32Spec;

    impl KeyedVecSpec for I32Spec {
        type Item = i32;
        type StorageKey = i32;
        type Lookup = super::super::keyed_vec::internal::CopyLookup;
        fn get_key(item: &i32) -> i32 {
            *item
        }
        fn key_extractor() -> &'static str {
            ""
        }
        fn default_item() -> &'static i32 {
            static D: i32 = 0;
            &D
        }
    }

    fn i32_keyed_vec(items: Vec<i32>) -> KeyedVec<I32Spec> {
        KeyedVec::new(items)
    }

    #[test]
    fn keyed_array_to_json_dense_empty() {
        let s = keyed_array_serializer::<I32Spec>(int32_serializer());
        assert_eq!(s.to_json(&i32_keyed_vec(vec![]), false), "[]");
    }

    #[test]
    fn keyed_array_to_json_dense_nonempty() {
        let s = keyed_array_serializer::<I32Spec>(int32_serializer());
        assert_eq!(s.to_json(&i32_keyed_vec(vec![10, 20]), false), "[10,20]");
    }

    #[test]
    fn keyed_array_to_json_readable_nonempty() {
        let s = keyed_array_serializer::<I32Spec>(int32_serializer());
        assert_eq!(s.to_json(&i32_keyed_vec(vec![1, 2]), true), "[\n  1,\n  2\n]");
    }

    #[test]
    fn keyed_array_from_json_array() {
        let s = keyed_array_serializer::<I32Spec>(int32_serializer());
        let result = s.from_json("[10,20,30]", false).unwrap();
        assert_eq!(&result[..], &[10_i32, 20, 30]);
    }

    #[test]
    fn keyed_array_from_json_null_is_empty() {
        let s = keyed_array_serializer::<I32Spec>(int32_serializer());
        assert!(s.from_json("null", false).unwrap().is_empty());
    }

    #[test]
    fn keyed_array_encode_empty_is_wire_246() {
        let s = keyed_array_serializer::<I32Spec>(int32_serializer());
        assert_eq!(s.to_bytes(&i32_keyed_vec(vec![])), b"skir\xf6");
    }

    #[test]
    fn keyed_array_encode_nonempty() {
        let s = keyed_array_serializer::<I32Spec>(int32_serializer());
        let bytes = s.to_bytes(&i32_keyed_vec(vec![5, 6]));
        // wire 247 + count 2 + items [5, 6]
        assert_eq!(&bytes[4..], &[0xf7_u8, 2, 5, 6]);
    }

    #[test]
    fn keyed_array_binary_round_trip() {
        let s = keyed_array_serializer::<I32Spec>(int32_serializer());
        for v in [vec![], vec![0_i32], vec![1, 2, 3]] {
            let kv = i32_keyed_vec(v.clone());
            let decoded = s.from_bytes(&s.to_bytes(&kv), false).unwrap();
            assert_eq!(&decoded[..], &v);
        }
    }

    // ── optional_serializer ───────────────────────────────────────────────────

    #[test]
    fn optional_to_json_none_is_null() {
        assert_eq!(optional_serializer(int32_serializer()).to_json(&None, false), "null");
    }

    #[test]
    fn optional_to_json_some_delegates() {
        assert_eq!(
            optional_serializer(int32_serializer()).to_json(&Some(42_i32), false),
            "42",
        );
    }

    #[test]
    fn optional_to_json_readable_none_is_null() {
        assert_eq!(optional_serializer(int32_serializer()).to_json(&None, true), "null");
    }

    #[test]
    fn optional_to_json_readable_some_delegates() {
        assert_eq!(
            optional_serializer(int32_serializer()).to_json(&Some(42_i32), true),
            "42",
        );
    }

    #[test]
    fn optional_from_json_null_is_none() {
        assert_eq!(
            optional_serializer(int32_serializer()).from_json("null", false).unwrap(),
            None::<i32>,
        );
    }

    #[test]
    fn optional_from_json_value_is_some() {
        assert_eq!(
            optional_serializer(int32_serializer()).from_json("7", false).unwrap(),
            Some(7_i32),
        );
    }

    #[test]
    fn optional_encode_none_is_wire_255() {
        assert_eq!(
            optional_serializer(int32_serializer()).to_bytes(&None),
            b"skir\xff",
        );
    }

    #[test]
    fn optional_encode_some_delegates() {
        // Some(5) → the inner int32 adapter writes wire byte 5 directly.
        let bytes = optional_serializer(int32_serializer()).to_bytes(&Some(5_i32));
        assert_eq!(&bytes[4..], &[5_u8]);
    }

    #[test]
    fn optional_binary_round_trip() {
        let s = optional_serializer(int32_serializer());
        for v in [None, Some(0_i32), Some(42), Some(-1)] {
            assert_eq!(s.from_bytes(&s.to_bytes(&v), false).unwrap(), v);
        }
    }
}
