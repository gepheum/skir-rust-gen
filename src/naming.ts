import { type RecordLocation } from "skir-internal";

export function toRustPathPrefix(modulePath: string): string {
  return "crate::skirout::base::".concat(
    modulePath
      .replace(/^@/, "external/")
      .replace(/-/g, "_")
      .replace(/\.skir$/, "")
      .replace(/\//g, "::"),
  );
}

/** Returns the name of the Rust type for the given record. */
export function getTypeName(record: RecordLocation): string {
  return record.recordAncestors
    .map((r) => upperCasedNameToIdentifier(r.name.text))
    .join("_");
}

export function toStructFieldName(input: string): string {
  return RESERVED_KEYWORDS.has(input) || input === "clone"
    ? `${input}_`
    : input;
}

export function upperCasedNameToIdentifier(input: string): string {
  return input === "Self" ? "Self_" : input;
}

const RESERVED_KEYWORDS = new Set<string>([
  "Self",
  // Strict keywords
  "as",
  "break",
  "const",
  "continue",
  "crate",
  "else",
  "enum",
  "extern",
  "false",
  "fn",
  "for",
  "if",
  "impl",
  "in",
  "let",
  "loop",
  "match",
  "mod",
  "move",
  "mut",
  "pub",
  "ref",
  "return",
  "self",
  "static",
  "struct",
  "super",
  "trait",
  "true",
  "type",
  "unsafe",
  "use",
  "where",
  "while",
  // Reserved keywords (not yet used but reserved)
  "abstract",
  "become",
  "box",
  "do",
  "final",
  "macro",
  "override",
  "priv",
  "typeof",
  "unsized",
  "virtual",
  "yield",
  // 2018+ keywords
  "async",
  "await",
  "dyn",
  "try",
]);
