//go:build dtw_core_noauth || dtw_poc_noauth

package helper

// The managed native runtime omits DTW's own authorizer.
// Native macOS/Windows permission checks remain in their respective drivers.
func pocCoreWithoutGrants(c Config) bool { return c.Managed }
