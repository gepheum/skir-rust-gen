import type { RecordKey, RecordLocation, ResolvedType } from "skir-internal";
import {
  getRustKeySpecSuffix,
  keyTypeIsSupported,
} from "./keyed_array_context.js";
import { getTypeName, Namer, toRustPathPrefix } from "./naming.js";

/**
 * Transforms a type found in a `.skir` file into a Go type.
 */
export class TypeSpeller {
  constructor(
    readonly recordMap: ReadonlyMap<RecordKey, RecordLocation>,
    readonly namer: Namer,
  ) {}

  getRustType(type: ResolvedType): string {
    switch (type.kind) {
      case "record": {
        const recordLocation = this.recordMap.get(type.key)!;
        const className = getTypeName(recordLocation);
        if (recordLocation.modulePath === this.namer.skirModule?.path) {
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
          return `${this.namer.vec}<${itemType}>`;
        }
      }
      case "optional": {
        const otherType = this.getRustType(type.other);
        return `${this.namer.option}<${otherType}>`;
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
            return this.namer.string;
          case "hash64":
            return "u64";
          case "timestamp":
            return "std::time::SystemTime";
          case "bytes":
            return this.namer.vec.concat("<u8>");
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
          return this.namer.vec.concat("::default()");
        }
      }
      case "optional": {
        return "None";
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
            return this.namer.string.concat("::new()");
          case "hash64":
            return "0_u64";
          case "timestamp":
            return "::std::time::SystemTime::UNIX_EPOCH";
          case "bytes":
            return this.namer.vec.concat("::default()");
        }
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
        if (recordLocation.modulePath === this.namer.skirModule?.path) {
          return `${className}_serializer()`;
        } else {
          const packageAlias = recordLocation.modulePath;
          return `${packageAlias}.${className}_serializer()`;
        }
      }
    }
  }
}

export function skirDefaultIsRustDefault(type: ResolvedType): boolean {
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
