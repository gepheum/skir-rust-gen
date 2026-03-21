import { type Field, type RecordLocation, convertCase } from "skir-internal";

export function toRustPathPrefix(modulePath: string): string {
  return "crate::skirout::base::".concat(
    modulePath
      .replace(/^@/, "external/")
      .replace(/-/g, "_")
      .replace(/\.skir$/, "")
      .replace(/\//g, "::"),
  );
}

export function structFieldToGetterName(field: Field | string): string {
  const skirName = typeof field === "string" ? field : field.name.text;
  const upperCamel = convertCase(skirName, "UpperCamel");
  return skirName.startsWith("search_") ||
    skirName === "string" ||
    skirName === "to_builder"
    ? upperCamel.concat("_")
    : upperCamel;
}

/** Returns the name of the Rust type for the given record. */
export function getTypeName(record: RecordLocation): string {
  return record.recordAncestors.map((r) => r.name.text).join("_");
}
