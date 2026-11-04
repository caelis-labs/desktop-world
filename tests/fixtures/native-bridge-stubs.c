// Keep the imported C symbols in a separate translation unit from Swift's
// @_cdecl controls. Importing and defining one symbol in the same Swift module
// trips older Swift SIL linkage assertions. This shim also exercises the real
// C ABI rather than replacing the product's calls with Swift-only calls.
#include "../../cmd/dtw/src/backends/macos/NativeBridge.h"

extern bool dtw_control_set_front_process(int32_t, uint32_t);
extern bool dtw_control_set_visual_front_process(int32_t, uint32_t);
extern int32_t dtw_control_front_process_matches(int32_t);
extern int32_t dtw_control_front_process_pid(void);
extern char *dtw_control_execution_scope_check(void);
extern void dtw_control_execution_scope_free(char *);
extern bool dtw_control_execution_scope_active(void);
extern char *dtw_control_scroll_anchor_check(void);
extern void dtw_control_scroll_anchor_free(char *);
extern void dtw_control_scroll_anchor_posted(void);

bool dtw_set_front_process(int32_t pid, uint32_t window) {
    return dtw_control_set_front_process(pid, window);
}
bool dtw_set_visual_front_process(int32_t pid, uint32_t window) {
    return dtw_control_set_visual_front_process(pid, window);
}
int32_t dtw_front_process_matches(int32_t pid) {
    return dtw_control_front_process_matches(pid);
}
int32_t dtw_front_process_pid(void) { return dtw_control_front_process_pid(); }
char *dtw_execution_scope_check(void) { return dtw_control_execution_scope_check(); }
void dtw_execution_scope_free(char *value) { dtw_control_execution_scope_free(value); }
bool dtw_execution_scope_active(void) { return dtw_control_execution_scope_active(); }
char *dtw_scroll_anchor_check(void) { return dtw_control_scroll_anchor_check(); }
void dtw_scroll_anchor_free(char *value) { dtw_control_scroll_anchor_free(value); }
void dtw_scroll_anchor_posted(void) { dtw_control_scroll_anchor_posted(); }
