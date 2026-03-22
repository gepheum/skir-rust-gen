import { FieldPath, Module, RecordKey, ResolvedType } from "skir-internal";

/** Pre-computed index of all keyed arrays found across the schema modules. */
export interface KeyedArrayContext {
  /**
   * Maps each record type (used as a keyed-array item) to the set of key
   * extractors declared for it. Each extractor is identified by its
   * dot-separated field path (e.g. `"owner.id"`) and carries the full
   * `FieldPath`.
   */
  readonly recordKeyToKeyExtractors: ReadonlyMap<
    RecordKey,
    ReadonlyMap<string, FieldPath>
  >;
  /** Record keys of enum types that appear as the key type of a keyed array. */
  readonly enumsUsedAsKeys: ReadonlySet<RecordKey>;
}

/** Scans all modules and builds a {@link KeyedArrayContext} from their keyed arrays. */
export function createKeyedArrayContext(
  skirModules: readonly Module[],
): KeyedArrayContext {
  const recordKeyToKeyExtractors = new Map<RecordKey, Map<string, FieldPath>>();
  const enumsUsedAsKeys = new Set<RecordKey>();
  const processType = (type: ResolvedType | undefined): void => {
    if (type?.kind !== "array" || !type.key) return;
    const keyExtractor = type.key.path.map((part) => part.name.text).join(".");
    const { item } = type;
    if (item.kind !== "record") {
      throw new TypeError();
    }
    const keyExtractors =
      recordKeyToKeyExtractors.get(item.key) ?? new Map<string, FieldPath>();
    if (keyExtractors.size <= 0) {
      recordKeyToKeyExtractors.set(item.key, keyExtractors);
    }
    keyExtractors.set(keyExtractor, type.key);
    if (type.key.keyType.kind === "record") {
      enumsUsedAsKeys.add(type.key.keyType.key);
    }
  };
  for (const skirModule of skirModules) {
    for (const record of skirModule.records) {
      for (const field of record.record.fields) {
        processType(field.type);
      }
    }
    skirModule.constants.forEach((constant) => {
      processType(constant.type);
    });
    skirModule.methods.forEach((method) => {
      processType(method.requestType);
      processType(method.responseType);
    });
  }
  return { recordKeyToKeyExtractors, enumsUsedAsKeys };
}
