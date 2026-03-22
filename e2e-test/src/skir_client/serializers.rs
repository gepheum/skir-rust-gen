use super::reflection::{PrimitiveType, TypeDescriptor};
use super::serializer::{Serializer, TypeAdapter};

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
            // Any string other than "0" is truthy, mirroring Go `string(sb) != "0"`.
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

/// Returns a [`Serializer`] for `bool` values.
pub fn bool_serializer() -> Serializer<bool> {
    Serializer::new(BoolAdapter)
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
        // Go: string(sb) != "0" — the string "0" is the only falsy string.
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
}
