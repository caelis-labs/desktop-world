//go:build windows && amd64

package windows

import (
	"encoding/binary"
	"strings"
	"testing"
	"time"

	dw "github.com/caelis-labs/desktop-world/internal/world"
)

func TestTextEventsUnicodeAndControlKeys(t *testing.T) {
	events := textEvents("中🌍\r\n\tX")
	want := []uint16{0x4e2d, 0xd83c, 0xdf0d, 13, 9, 'X'}
	// One BMP character, one UTF-16 surrogate pair, Enter, Tab and X.
	if len(events) != len(want)*2 {
		t.Fatalf("event count=%d", len(events))
	}
	for i, unit := range want {
		down, up := events[i*2], events[i*2+1]
		key := binary.LittleEndian.Uint16(down.Data[:2])
		scan := binary.LittleEndian.Uint16(down.Data[2:4])
		flags := binary.LittleEndian.Uint32(down.Data[4:8])
		if unit == 13 || unit == 9 {
			if key != unit || scan != 0 || flags != 0 {
				t.Fatalf("control key=%d scan=%d flags=%d", key, scan, flags)
			}
		} else if key != 0 || scan != unit || flags != 4 {
			t.Fatalf("Unicode key=%d scan=%d flags=%d", key, scan, flags)
		}
		if binary.LittleEndian.Uint32(up.Data[4:8]) != flags|2 {
			t.Fatal("unpaired key release")
		}
	}
}
func TestCooperativeBurstLimitsCountUTF16(t *testing.T) {
	for _, tc := range []struct {
		text string
		want bool
	}{
		{text: string(make([]rune, 256)), want: false},
		{text: string(make([]rune, 257)), want: true},
		{text: "🌍", want: false},
		{text: strings.Repeat("🌍", 128), want: false},
		{text: strings.Repeat("🌍", 129), want: true},
	} {
		if got := inputBurstError(dw.Step{TypeText: &dw.TypeText{Text: tc.text}}) != nil; got != tc.want {
			t.Fatal("text burst limit", got, tc.want)
		}
	}
	for _, duration := range []time.Duration{500 * time.Millisecond, 501 * time.Millisecond} {
		if got := inputBurstError(dw.Step{Drag: &dw.Drag{Duration: duration}}) != nil; got != (duration > 500*time.Millisecond) {
			t.Fatal("drag burst limit")
		}
	}
}
