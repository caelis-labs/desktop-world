package main

import (
	"context"
	dw "github.com/caelis-labs/desktop-world"
	"github.com/caelis-labs/desktop-world/local"
)

type funcOpen func(context.Context, local.Options) (dw.World, error)
