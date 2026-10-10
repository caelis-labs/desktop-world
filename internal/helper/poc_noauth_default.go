//go:build !dtw_core_noauth && !dtw_poc_noauth

package helper

func pocCoreWithoutGrants(Config) bool { return false }
