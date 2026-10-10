//go:build dtw_poc_noauth

package helper

// Only the explicitly tagged, managed POC helper omits DTW's own authorizer.
// Native macOS/Windows permission checks remain in their respective drivers.
func pocCoreWithoutGrants(c Config) bool { return c.Managed }
