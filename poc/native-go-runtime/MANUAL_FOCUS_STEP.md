# Owned minimal input fixture

The fixture is a single standard AppKit `NSTextField`. Its log records only
timestamps, focus state, event type/key code when enabled, and text length. It
never stores the input text.

Build only (does not launch or change focus):

```sh
./poc/native-go-runtime/build-minimal-input-fixture.sh /private/tmp/dtw-minimal-input-check plain
./poc/native-go-runtime/build-minimal-input-fixture.sh /private/tmp/dtw-minimal-input-check deferred
```

Open exactly one bundle manually in Finder: `DTWMinimalInput.app` for the plain
control or `DTWDeferredEventInput.app` for the corrected `sendEvent` variant.
Check the window title, click its sole input field, and type a non-sensitive
digit. A successful run displays the digit and writes a `text_change` row to
`/private/tmp/dtw-minimal-input-<PID>.jsonl`. The deferred variant also writes
`window_event` rows. Close the app before opening the other bundle to prevent
window confusion.

The 2026-10-10 human round already passed both the plain control and the
corrected deferred variant. The intermediate version that read
`NSEvent.eventNumber` for keyboard events crashed and must not be used.
