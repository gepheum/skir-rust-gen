import type { RecordKey, RecordLocation, ResolvedType } from "skir-internal";
import {
  getRustKeySpecSuffix,
  keyTypeIsSupported,
} from "./keyed_array_context.js";
import { getTypeName, toRustPathPrefix } from "./naming.js";

/**
 * Transforms a type found in a `.skir` file into a Go type.
 */
export class TypeSpeller {
  constructor(
    readonly recordMap: ReadonlyMap<RecordKey, RecordLocation>,
    readonly skirModulePath: string | undefined,
  ) {}

  getRustType(type: ResolvedType): string {
    switch (type.kind) {
      case "record": {
        const recordLocation = this.recordMap.get(type.key)!;
        const className = getTypeName(recordLocation);
        if (recordLocation.modulePath === this.skirModulePath) {
          return className;
        } else {
          const rustPathPrefix = toRustPathPrefix(recordLocation.modulePath);
          return `${rustPathPrefix}::${className}`;
        }
      }
      case "array": {
        const itemType = this.getRustType(type.item);
        if (type.key && keyTypeIsSupported(type.key.keyType)) {
          const suffix = getRustKeySpecSuffix(type.key);
          return `crate::skir_client::keyed_vec::KeyedVec<${itemType}${suffix}>`;
        } else {
          return `std::vec::Vec<${itemType}>`;
        }
      }
      case "optional": {
        const otherType = this.getRustType(type.other);
        return `std::option::Option<${otherType}>`;
      }
      case "primitive": {
        const { primitive } = type;
        switch (primitive) {
          case "bool":
            return "bool";
          case "int32":
            return "i32";
          case "int64":
            return "i64";
          case "float32":
            return "f32";
          case "float64":
            return "f64";
          case "string":
            return "std::string::String";
          case "hash64":
            return "u64";
          case "timestamp":
            return "std::time::SystemTime";
          case "bytes":
            return "std::vec::Vec<u8>";
        }
      }
    }
  }

  getDefaultExpr(type: ResolvedType): string {
    switch (type.kind) {
      case "record": {
        const rustType = this.getRustType(type);
        return `${rustType}::default()`;
      }
      case "array": {
        if (type.key && keyTypeIsSupported(type.key.keyType)) {
          return "crate::skir_client::keyed_vec::KeyedVec::default()";
        } else {
          return "std::vec::Vec::default()";
        }
      }
      case "optional": {
        return "std::option::Option::None";
      }
      case "primitive": {
        const { primitive } = type;
        switch (primitive) {
          case "bool":
            return "false";
          case "int32":
            return "0_i32";
          case "int64":
            return "0_i64";
          case "float32":
            return "0.0_f32";
          case "float64":
            return "0.0_f64";
          case "string":
            return "std::string::String::new()";
          case "hash64":
            return "0_u64";
          case "timestamp":
            return "std::time::SystemTime::UNIX_EPOCH";
          case "bytes":
            return "std::vec::Vec::new()";
        }
      }
    }
  }

  /**
   * Returns true if the skir default value for the given type is the same as
   * what Rust's Default trait would produce, meaning the type can participate
   * in a #[derive(Default)] without a manual impl.
   */
  skirDefaultIsRustDefault(type: ResolvedType): boolean {
    switch (type.kind) {
      case "record":
        // Enums derive Default via #[default] on Unknown.
        // Structs either derive or manually impl Default — either way
        // ::default() is the skir default.
        return true;
      case "array":
        // Vec::default() == Vec::new()
        return true;
      case "optional":
        // Option::default() == None
        return true;
      case "primitive":
        switch (type.primitive) {
          case "bool":
          case "int32":
          case "int64":
          case "float32":
          case "float64":
          case "string":
          case "hash64":
          case "bytes":
            return true;
          case "timestamp":
            // SystemTime does not implement Default in std.
            return false;
        }
    }
  }

  getTypeName(recordKey: RecordKey): string {
    const record = this.recordMap.get(recordKey)!;
    return getTypeName(record);
  }

  getSerializerExpression(type: ResolvedType): string {
    switch (type.kind) {
      case "primitive": {
        switch (type.primitive) {
          case "bool":
            return "skir_client.BoolSerializer()";
          case "int32":
            return "skir_client.Int32Serializer()";
          case "int64":
            return "skir_client.Int64Serializer()";
          case "hash64":
            return "skir_client.Hash64Serializer()";
          case "float32":
            return "skir_client.Float32Serializer()";
          case "float64":
            return "skir_client.Float64Serializer()";
          case "timestamp":
            return "skir_client.TimestampSerializer()";
          case "string":
            return "skir_client.StringSerializer()";
          case "bytes":
            return "skir_client.BytesSerializer()";
        }
        const _: never = type.primitive;
        throw TypeError();
      }
      case "array": {
        if (type.key) {
          const keyExtractor = type.key.path
            .map((part) => part.name.text)
            .join(".");
          return (
            "skir_client.Internal__ArraySerializer(\n" +
            this.getSerializerExpression(type.item) +
            ",\n" +
            JSON.stringify(keyExtractor) +
            ",\n)"
          );
        } else {
          return (
            "skir_client.ArraySerializer(\n" +
            this.getSerializerExpression(type.item) +
            ",\n)"
          );
        }
      }
      case "optional": {
        return (
          "skir_client.OptionalSerializer(\n" +
          this.getSerializerExpression(type.other) +
          ",\n)"
        );
      }
      case "record": {
        const recordLocation = this.recordMap.get(type.key)!;
        const className = getTypeName(recordLocation);
        if (recordLocation.modulePath === this.skirModulePath) {
          return `${className}_serializer()`;
        } else {
          const packageAlias = recordLocation.modulePath;
          return `${packageAlias}.${className}_serializer()`;
        }
      }
    }
  }
}

function skirDefaultIsRustDefault(type: ResolvedType): boolean {
  switch (type.kind) {
    case "record":
    case "array":
      return true;
    case "optional":
      return skirDefaultIsRustDefault(type.other);
    case "primitive": {
      const { primitive } = type;
      switch (primitive) {
        case "timestamp":
          return false;
        default:
          return false;
      }
    }
  }
}
