#ifndef DTW_NATIVE_BRIDGE_H
#define DTW_NATIVE_BRIDGE_H

#include <stdbool.h>
#include <stdint.h>

// Rust exports use extern "C". Import their actual C calling convention into
// Swift instead of declaring Swift-convention functions with _silgen_name.
bool dtw_set_front_process(int32_t pid, uint32_t window);
bool dtw_set_visual_front_process(int32_t pid, uint32_t window);
int32_t dtw_front_process_matches(int32_t pid);
int32_t dtw_front_process_pid(void);

char *dtw_execution_scope_check(void);
void dtw_execution_scope_free(char *value);
bool dtw_execution_scope_active(void);
char *dtw_scroll_anchor_check(void);
void dtw_scroll_anchor_free(char *value);
void dtw_scroll_anchor_posted(void);

#endif
