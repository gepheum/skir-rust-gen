import { Module, type RecordLocation } from "skir-internal";

export class Namer {
  constructor(readonly skirModule: Module | null) {}

  readonly box = this.maybeQualify("std::boxed::", "Box");
  readonly clone = this.maybeQualify("std::clone::", "Clone");
  readonly copy = this.maybeQualify("std::marker::", "Copy");
  readonly debug = this.maybeQualify("std::fmt::", "Debug");
  readonly default = this.maybeQualify("std::default::", "Default");
  readonly eq = this.maybeQualify("std::cmp::", "Eq");
  readonly hash = this.maybeQualify("std::hash::", "Hash");
  readonly option = this.maybeQualify("std::option::", "Option");
  readonly partialEq = this.maybeQualify("std::cmp::", "PartialEq");
  readonly string = this.maybeQualify("std::string::", "String");
  readonly vec = this.maybeQualify("std::vec::", "Vec");

  private maybeQualify(pathPrefix: string, name: string): string {
    return this.skirModule?.nameToDeclaration[name]
      ? `${pathPrefix}${name}`
      : name;
  }
}

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
    .map((r) => maybeEscapeUpperCasedName(r.name.text))
    .join("_");
}

export function toStructFieldName(input: string): string {
  return RESERVED_KEYWORDS.has(input) || GENERARATED_STRUCT_METHODS.has(input)
    ? `${input}_`
    : input;
}

export function maybeEscapeUpperCasedName(input: string): string {
  return isUpperCasedKeyword(input) ? `${input}_` : input;
}

export function isUpperCasedKeyword(input: string): boolean {
  return input === "Self";
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

const GENERARATED_STRUCT_METHODS = new Set<string>([
  "clone",
  "default",
  "default_ref",
  "fmt",
  "serializer",
]);
