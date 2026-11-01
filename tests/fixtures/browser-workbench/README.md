# Owned Chrome reliability fixture

The default `/` inventory page retains its 240 records and original acceptance
workflow. `/reliability` is a separate local page for investigating Windows input
delivery and accessibility traversal. Nothing is sent to an external website.
These fixtures do not prove arbitrary website compatibility or reproduce
GitHub's implementation.

Start a fresh case from source:

```powershell
python tests/fixtures/browser-workbench/server.py --output "D:\Temp\DTW issue82 DSH" --case issue82-dsh
```

The first stdout line contains the loopback URL. Append `reliability` to open the
probe page. Use a new output directory/case for independent runs. Creating an
empty `stop` file inside the output directory stops only this server.

## Input delivery

For each comparison, give **Run label** a distinct value identifying the actual
DTW route under test. Labels do not force or prove that route; retain the DTW
delivery result alongside the fixture's journal. Observe fresh state before each
input and perform one action before checking the result.

1. In normal mode, invoke **Create test record** once. A successful submission
   produces a receipt containing an attempt sequence, `accepted`, submission
   count and activation nonce. Verify exactly one new accepted submission.
2. Compare DTW coordinate input with UIA `press`. Also test the actual foreground
   input route. The fixture cannot force DTW to choose one of its delivery routes.
   Retain failures and compare `pointerdown/up`, `mousedown/up`, `click`, focus,
   `detail`, `buttons`, trust flags and submit events. API dispatch success alone
   does not establish a submission effect.
3. Enable **Require trusted pointer sequence** to demonstrate a control whose
   handler requires a fresh trusted pointer down/up/click sequence. A semantic
   invocation or keyboard submit can legitimately be rejected in this mode;
   that rejection alone is not a DTW regression. Normal mode remains the check
   for an ordinary form submit.
4. **Settle for 1500 ms** disables the submit button temporarily. Wait for the
   visible `ready` state before the next intended submit. This separates disabled
   controls and loading from delivery failures.
5. Check the unnamed plus icon and paragraph fragments through the same native
   accessibility path. The provider may expose or combine text fragments
   differently; no visual paragraph reconstruction is assumed.

Each real submit activation generates a new nonce. A repeated HTTP request with
the same nonce is recorded as a duplicate attempt without another effect. Two
actual activations have distinct nonces and produce two effects, so this does
not hide repeated-click bugs. Do not replay an action merely because its first
effect is delayed or uncertain.

## Readback and limits

The supervisor reads `<output>/state.json` as UTF-8. Original inventory state is
unchanged. New evidence is under `reliability`:

- `events`: ordered, timestamped input/focus/ready events, including the run label
  and page-frame identifier;
- `attempts`: every submit request, with acceptance/rejection reason and nonce;
- `submissions`: accepted effects only;
- `icon_activations`: the unnamed icon's activation count.

`GET /api/reliability` returns counts plus the last 20 attempts and 40 events.
The **Refresh diagnostic counts** button renders those counts in the page after
pending journal writes. This endpoint does not expose inventory records, notes,
attachments or the complete supervisor file. Full effect assertions should use
the persisted journal. Trust flags describe the browser's event report; they
are not an authenticated operating-system input source identity.

The page serializes journal requests to preserve browser-event order. Movement
events are sampled at most once per 100 ms per type; down/up/click events are
never coalesced. A visible journal failure is a failed evidence capture, not proof
that an input action had no effect. Each case is bounded to 10,000 reliability
events and 1,000 submit attempts; use a fresh case after reaching a limit.

## Traversal

Use **Load 360-link catalogue** after the input-delivery cases. It adds 360 links
named `Fixture link 001` through `Fixture link 360`, arranged in eight named
groups; after catalogue ready the links do not rerender. Keeping this case
separate prevents the intentionally large tree from blocking a selector preflight
before input delivery can be measured. Compare fully paginated filtered and unfiltered
observations of the same window/subtree. A partial first page, provider read gap,
or exhausted deadline cannot establish that omitted links do not exist. Preserve
coverage information rather than treating partial results as complete.

## Non-GUI checks

```powershell
python tests/fixtures/browser-workbench/test_server.py
```

The checks use standard-library HTTP clients against an ephemeral loopback server
and temporary files. They do not launch Chrome, synthesize UI input or certify a
native DTW route. Native and DSH/Antigravity runs are separate acceptance steps.

## Opt-in Windows DTW probe

From the source checkout, with Chrome installed and the interactive desktop idle:

```powershell
python -X utf8 tests/acceptance/windows/browser_reliability_probe.py --dtw "C:\path\to\dtw.exe" --chrome "C:\Program Files\Google\Chrome\Application\chrome.exe" --output "D:\Temp\DTW reliability new-case"
```

This starts an owned Chrome profile and local fixture, and drives input only
through the selected DTW binary's normal MCP tools. It compares semantic press
and pixel click in both modes, then loads and fully paginates the link catalogue.
Pillow locates the uniquely colored submit button in DTW's actual PNG; no DOM or
browser-driver input is used. It retains raw requests/results, early and final
fixture journals, actual route receipts and cleanup outcomes. Explicit focus
prepares baseline captures; it does not force the later click's delivery route.
The probe does not certify automatic capture preparation or model vision.

`--skip-traversal` collects only the four input cases; its result explicitly marks
traversal unverified. In full mode an incomplete traversal cannot pass the set
comparison, even if input succeeds. Trust/primary/button attributes are recorded
as diagnostics, not invented correctness gates. In particular, a `click` event's
`isPrimary=false` is a specified default and is not evidence of a missing primary
mouse down; see [Pointer Events, event attributes](https://www.w3.org/TR/pointerevents/latest/#event-attributes).

Foreign windows, including another automation coordinator's full-desktop cursor
overlay, can make the current GDI capture refuse occluded content. Finish or
dispose that coordinator through its normal lifecycle before this probe. The
probe neither disables foreign windows nor exempts them from capture proof.

## Actual model image-channel check

Use the same server's `/vision` page in a fresh case for each Agent host. It
renders a random eight-character challenge in a canvas with a generic accessible
name. The browser receives only a server-rendered raster PNG: HTML, scripts,
fallback text, titles, accessibility labels and JSON APIs contain no answer.
The PNG has only IHDR/IDAT/IEND chunks, without text metadata. Rendering uses a
large fixed raster font and an alphabet that excludes confusing character pairs.
No additional Python or browser dependency is required.

Ask the Agent to use **DTW capture only** to read the picture, enter its eight
characters into **Visual token**, and press **Verify picture** once. Do not give
it the supervisor's expected token, fixture files or a different screenshot/OCR
tool. Wait for **Picture ready** before its capture. A successful inventory edit,
an image-preview UI error or a host assertion that it can see images does not
replace this check.

The supervisor alone reads `state.json.vision.expected_token` and
`state.json.vision.attempts`. Every valid eight-character alphanumeric submission
records the submitted value, pass/fail and timestamp; the page receives only
pass/fail and attempt count. Inputs are bounded to eight characters, and each
fresh case allows at most eight attempts. Retain failed attempts and verify
the submitted value independently. The token stays fixed within one case;
refreshing the page does not create a new one. Both `/vision` and its PNG return
404 from old instrumented layouts that copied only server.py/index.html.
