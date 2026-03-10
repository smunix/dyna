package dynago

import (
	"encoding/json"
	"fmt"
	"sort"
	"strings"
)

// ---------------------------------------------------------------------------
// JSON Diff — compute RFC 6902 operations between two JSON values
// ---------------------------------------------------------------------------

// Diff computes the JSON Patch operations needed to transform oldVal into newVal.
func Diff(oldVal, newVal json.RawMessage) ([]PatchOperation, error) {
	var oldParsed, newParsed interface{}
	if err := json.Unmarshal(oldVal, &oldParsed); err != nil {
		return nil, fmt.Errorf("diff: invalid old JSON: %w", err)
	}
	if err := json.Unmarshal(newVal, &newParsed); err != nil {
		return nil, fmt.Errorf("diff: invalid new JSON: %w", err)
	}
	var ops []PatchOperation
	diffRecursive("", oldParsed, newParsed, &ops)
	return ops, nil
}

func diffRecursive(path string, oldVal, newVal interface{}, ops *[]PatchOperation) {
	if jsonEqual(oldVal, newVal) {
		return
	}

	oldMap, oldIsMap := oldVal.(map[string]interface{})
	newMap, newIsMap := newVal.(map[string]interface{})

	if oldIsMap && newIsMap {
		// Collect all keys
		allKeys := map[string]bool{}
		for k := range oldMap {
			allKeys[k] = true
		}
		for k := range newMap {
			allKeys[k] = true
		}
		sorted := make([]string, 0, len(allKeys))
		for k := range allKeys {
			sorted = append(sorted, k)
		}
		sort.Strings(sorted)

		for _, key := range sorted {
			childPath := path + "/" + escapeJSONPointer(key)
			oldChild, inOld := oldMap[key]
			newChild, inNew := newMap[key]

			if inOld && !inNew {
				*ops = append(*ops, PatchOperation{Op: OpRemove, Path: childPath})
			} else if !inOld && inNew {
				val, _ := json.Marshal(newChild)
				*ops = append(*ops, PatchOperation{Op: OpAdd, Path: childPath, Value: val})
			} else {
				diffRecursive(childPath, oldChild, newChild, ops)
			}
		}
		return
	}

	oldArr, oldIsArr := oldVal.([]interface{})
	newArr, newIsArr := newVal.([]interface{})

	if oldIsArr && newIsArr {
		// Simple element-wise diff for arrays
		maxLen := len(oldArr)
		if len(newArr) > maxLen {
			maxLen = len(newArr)
		}
		for i := 0; i < maxLen; i++ {
			childPath := fmt.Sprintf("%s/%d", path, i)
			if i >= len(oldArr) {
				val, _ := json.Marshal(newArr[i])
				*ops = append(*ops, PatchOperation{Op: OpAdd, Path: childPath, Value: val})
			} else if i >= len(newArr) {
				// Remove from the end backwards to keep indices stable
				removePath := fmt.Sprintf("%s/%d", path, len(oldArr)-1-(i-len(newArr)))
				*ops = append(*ops, PatchOperation{Op: OpRemove, Path: removePath})
			} else {
				diffRecursive(childPath, oldArr[i], newArr[i], ops)
			}
		}
		return
	}

	// Scalar or type change → replace
	val, _ := json.Marshal(newVal)
	*ops = append(*ops, PatchOperation{Op: OpReplace, Path: path, Value: val})
}

// ---------------------------------------------------------------------------
// Apply Patch — apply RFC 6902 operations to a JSON document
// ---------------------------------------------------------------------------

// ApplyPatch applies a list of patch operations to a JSON document in place.
func ApplyPatch(doc *json.RawMessage, ops []PatchOperation) error {
	var parsed interface{}
	if err := json.Unmarshal(*doc, &parsed); err != nil {
		return fmt.Errorf("apply_patch: invalid JSON: %w", err)
	}

	for _, op := range ops {
		var err error
		parsed, err = applyOp(parsed, op)
		if err != nil {
			return err
		}
	}

	result, err := json.Marshal(parsed)
	if err != nil {
		return err
	}
	*doc = result
	return nil
}

func applyOp(doc interface{}, op PatchOperation) (interface{}, error) {
	switch op.Op {
	case OpAdd:
		var val interface{}
		if err := json.Unmarshal(op.Value, &val); err != nil {
			return doc, err
		}
		return setAtPath(doc, op.Path, val, false)

	case OpRemove:
		return removeAtPath(doc, op.Path)

	case OpReplace:
		var val interface{}
		if err := json.Unmarshal(op.Value, &val); err != nil {
			return doc, err
		}
		return setAtPath(doc, op.Path, val, true)

	case OpMove:
		val, newDoc, err := getAndRemoveAtPath(doc, op.From)
		if err != nil {
			return doc, err
		}
		return setAtPath(newDoc, op.Path, val, false)

	case OpCopy:
		val, err := getAtPath(doc, op.From)
		if err != nil {
			return doc, err
		}
		return setAtPath(doc, op.Path, val, false)

	case OpTest:
		var expected interface{}
		if err := json.Unmarshal(op.Value, &expected); err != nil {
			return doc, err
		}
		actual, err := getAtPath(doc, op.Path)
		if err != nil {
			return doc, err
		}
		if !jsonEqual(actual, expected) {
			return doc, fmt.Errorf("test failed at %s", op.Path)
		}
		return doc, nil

	default:
		return doc, fmt.Errorf("unknown operation: %s", op.Op)
	}
}

// ---------------------------------------------------------------------------
// Invert Operations — create inverse patch operations for revert
// ---------------------------------------------------------------------------

// InvertOperations creates the inverse of a set of patch operations.
// The snapshot parameter is the state *before* the original operations were applied.
func InvertOperations(ops []PatchOperation, snapshot json.RawMessage) ([]PatchOperation, error) {
	var doc interface{}
	if snapshot != nil {
		if err := json.Unmarshal(snapshot, &doc); err != nil {
			return nil, err
		}
	}

	inverted := make([]PatchOperation, 0, len(ops))
	// Process in reverse order
	for i := len(ops) - 1; i >= 0; i-- {
		op := ops[i]
		switch op.Op {
		case OpAdd:
			inverted = append(inverted, PatchOperation{Op: OpRemove, Path: op.Path})
		case OpRemove:
			// Need the old value from the snapshot
			oldVal, err := getAtPath(doc, op.Path)
			if err != nil {
				// If we can't find the old value, use null
				inverted = append(inverted, PatchOperation{
					Op:    OpAdd,
					Path:  op.Path,
					Value: json.RawMessage("null"),
				})
			} else {
				val, _ := json.Marshal(oldVal)
				inverted = append(inverted, PatchOperation{Op: OpAdd, Path: op.Path, Value: val})
			}
		case OpReplace:
			// Need the old value from the snapshot
			oldVal, err := getAtPath(doc, op.Path)
			if err != nil {
				inverted = append(inverted, PatchOperation{
					Op:    OpReplace,
					Path:  op.Path,
					Value: json.RawMessage("null"),
				})
			} else {
				val, _ := json.Marshal(oldVal)
				inverted = append(inverted, PatchOperation{Op: OpReplace, Path: op.Path, Value: val})
			}
		case OpMove:
			inverted = append(inverted, PatchOperation{Op: OpMove, From: op.Path, Path: op.From})
		case OpCopy, OpTest:
			// Copy and Test don't have meaningful inverses; skip
		}
	}
	return inverted, nil
}

// ---------------------------------------------------------------------------
// Three-way merge
// ---------------------------------------------------------------------------

// ThreeWayMerge merges local and remote changes against a common base.
// Returns the merged value and any conflicts.
func ThreeWayMerge(base, local, remote json.RawMessage) (json.RawMessage, []Conflict, error) {
	var baseParsed, localParsed, remoteParsed interface{}
	if err := json.Unmarshal(base, &baseParsed); err != nil {
		return nil, nil, err
	}
	if err := json.Unmarshal(local, &localParsed); err != nil {
		return nil, nil, err
	}
	if err := json.Unmarshal(remote, &remoteParsed); err != nil {
		return nil, nil, err
	}

	var conflicts []Conflict
	merged := mergeRecursive("", baseParsed, localParsed, remoteParsed, &conflicts)

	result, err := json.Marshal(merged)
	if err != nil {
		return nil, nil, err
	}
	return result, conflicts, nil
}

func mergeRecursive(path string, base, local, remote interface{}, conflicts *[]Conflict) interface{} {
	if jsonEqual(local, base) && jsonEqual(remote, base) {
		return base
	}
	if jsonEqual(local, base) {
		return remote
	}
	if jsonEqual(remote, base) {
		return local
	}
	if jsonEqual(local, remote) {
		return local
	}

	baseMap, baseIsMap := base.(map[string]interface{})
	localMap, localIsMap := local.(map[string]interface{})
	remoteMap, remoteIsMap := remote.(map[string]interface{})

	if baseIsMap && localIsMap && remoteIsMap {
		allKeys := map[string]bool{}
		for k := range baseMap {
			allKeys[k] = true
		}
		for k := range localMap {
			allKeys[k] = true
		}
		for k := range remoteMap {
			allKeys[k] = true
		}
		sorted := make([]string, 0, len(allKeys))
		for k := range allKeys {
			sorted = append(sorted, k)
		}
		sort.Strings(sorted)

		merged := map[string]interface{}{}
		for _, key := range sorted {
			childPath := path + "/" + key
			baseVal := mapGet(baseMap, key)
			localVal := mapGet(localMap, key)
			remoteVal := mapGet(remoteMap, key)
			mergedVal := mergeRecursive(childPath, baseVal, localVal, remoteVal, conflicts)
			if mergedVal != nil || mapHas(localMap, key) || mapHas(remoteMap, key) {
				merged[key] = mergedVal
			}
		}
		return merged
	}

	// Scalar or type conflict
	localJSON, _ := json.Marshal(local)
	remoteJSON, _ := json.Marshal(remote)
	baseJSON, _ := json.Marshal(base)
	*conflicts = append(*conflicts, Conflict{
		JSONPath:    path,
		LocalValue:  localJSON,
		RemoteValue: remoteJSON,
		BaseValue:   baseJSON,
	})
	return local // default to local
}

// ---------------------------------------------------------------------------
// JSON Pointer helpers
// ---------------------------------------------------------------------------

func parsePointer(path string) []string {
	if path == "" || path == "/" {
		return nil
	}
	parts := strings.Split(strings.TrimPrefix(path, "/"), "/")
	for i, p := range parts {
		parts[i] = unescapeJSONPointer(p)
	}
	return parts
}

func escapeJSONPointer(s string) string {
	s = strings.ReplaceAll(s, "~", "~0")
	s = strings.ReplaceAll(s, "/", "~1")
	return s
}

func unescapeJSONPointer(s string) string {
	s = strings.ReplaceAll(s, "~1", "/")
	s = strings.ReplaceAll(s, "~0", "~")
	return s
}

func getAtPath(doc interface{}, path string) (interface{}, error) {
	parts := parsePointer(path)
	current := doc
	for _, part := range parts {
		switch v := current.(type) {
		case map[string]interface{}:
			val, ok := v[part]
			if !ok {
				return nil, fmt.Errorf("path not found: %s", path)
			}
			current = val
		case []interface{}:
			idx, err := parseArrayIndex(part, len(v))
			if err != nil {
				return nil, err
			}
			current = v[idx]
		default:
			return nil, fmt.Errorf("cannot traverse into %T at %s", current, path)
		}
	}
	return current, nil
}

func setAtPath(doc interface{}, path string, value interface{}, mustExist bool) (interface{}, error) {
	parts := parsePointer(path)
	if len(parts) == 0 {
		return value, nil // replace root
	}

	parent := parts[:len(parts)-1]
	key := parts[len(parts)-1]

	target, err := navigateTo(doc, parent)
	if err != nil {
		return doc, err
	}

	switch v := target.(type) {
	case map[string]interface{}:
		if mustExist {
			if _, ok := v[key]; !ok {
				return doc, fmt.Errorf("path not found for replace: %s", path)
			}
		}
		v[key] = value
	case []interface{}:
		if key == "-" {
			// Append
			target := append(v, value)
			return replaceAtParent(doc, parent, target)
		}
		idx, err := parseArrayIndex(key, len(v)+1)
		if err != nil {
			return doc, err
		}
		if mustExist && idx >= len(v) {
			return doc, fmt.Errorf("index out of range for replace: %s", path)
		}
		if idx >= len(v) {
			target := append(v, value)
			return replaceAtParent(doc, parent, target)
		}
		if !mustExist {
			// Insert
			newArr := make([]interface{}, len(v)+1)
			copy(newArr, v[:idx])
			newArr[idx] = value
			copy(newArr[idx+1:], v[idx:])
			return replaceAtParent(doc, parent, newArr)
		}
		v[idx] = value
	default:
		return doc, fmt.Errorf("cannot set on %T", target)
	}
	return doc, nil
}

func removeAtPath(doc interface{}, path string) (interface{}, error) {
	parts := parsePointer(path)
	if len(parts) == 0 {
		return nil, nil
	}

	parent := parts[:len(parts)-1]
	key := parts[len(parts)-1]

	target, err := navigateTo(doc, parent)
	if err != nil {
		return doc, err
	}

	switch v := target.(type) {
	case map[string]interface{}:
		delete(v, key)
	case []interface{}:
		idx, err := parseArrayIndex(key, len(v))
		if err != nil {
			return doc, err
		}
		newArr := append(v[:idx], v[idx+1:]...)
		return replaceAtParent(doc, parent, newArr)
	default:
		return doc, fmt.Errorf("cannot remove from %T", target)
	}
	return doc, nil
}

func getAndRemoveAtPath(doc interface{}, path string) (interface{}, interface{}, error) {
	val, err := getAtPath(doc, path)
	if err != nil {
		return nil, doc, err
	}
	newDoc, err := removeAtPath(doc, path)
	if err != nil {
		return nil, doc, err
	}
	return val, newDoc, nil
}

func navigateTo(doc interface{}, parts []string) (interface{}, error) {
	current := doc
	for _, part := range parts {
		switch v := current.(type) {
		case map[string]interface{}:
			val, ok := v[part]
			if !ok {
				return nil, fmt.Errorf("path segment not found: %s", part)
			}
			current = val
		case []interface{}:
			idx, err := parseArrayIndex(part, len(v))
			if err != nil {
				return nil, err
			}
			current = v[idx]
		default:
			return nil, fmt.Errorf("cannot navigate into %T", current)
		}
	}
	return current, nil
}

func replaceAtParent(doc interface{}, parentPath []string, newValue interface{}) (interface{}, error) {
	if len(parentPath) == 0 {
		return newValue, nil
	}
	return setAtPath(doc, "/"+strings.Join(parentPath, "/"), newValue, true)
}

func parseArrayIndex(s string, maxLen int) (int, error) {
	if s == "-" {
		return maxLen, nil
	}
	var idx int
	if _, err := fmt.Sscanf(s, "%d", &idx); err != nil {
		return 0, fmt.Errorf("invalid array index: %s", s)
	}
	if idx < 0 || idx >= maxLen {
		return 0, fmt.Errorf("array index out of range: %d (len=%d)", idx, maxLen)
	}
	return idx, nil
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func jsonEqual(a, b interface{}) bool {
	aj, _ := json.Marshal(a)
	bj, _ := json.Marshal(b)
	return string(aj) == string(bj)
}

func mapGet(m map[string]interface{}, key string) interface{} {
	if m == nil {
		return nil
	}
	return m[key]
}

func mapHas(m map[string]interface{}, key string) bool {
	if m == nil {
		return false
	}
	_, ok := m[key]
	return ok
}
