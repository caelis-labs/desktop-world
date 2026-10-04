//go:build !dtw_background_poc || !darwin || !cgo

package main

import (
	"flag"
	"github.com/caelis-labs/desktop-world/local"
)

func backgroundPOCFlag(*flag.FlagSet) funcOpen { return local.Open }
