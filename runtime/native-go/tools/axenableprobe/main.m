// Bounded AX-role probe for one explicitly selected disposable Electron app.
// It never reads titles, values, text, paths, other apps, or user input. The
// optional manual AX flag changes only that target process' AX provider state;
// it never changes TCC settings or activates the app.
#import <ApplicationServices/ApplicationServices.h>
#import <CoreFoundation/CoreFoundation.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef struct {
  int visited, webAreas, buttons, textFields, staticTexts, childErrors;
} Stats;

static void walk(AXUIElementRef element, int depth, Stats *stats) {
  if (!element || depth <= 0 || stats->visited >= 400) return;
  stats->visited++;
  CFTypeRef role = NULL;
  if (AXUIElementCopyAttributeValue(element, kAXRoleAttribute, &role) == kAXErrorSuccess &&
      role && CFGetTypeID(role) == CFStringGetTypeID()) {
    if (CFStringCompare((CFStringRef)role, CFSTR("AXWebArea"), 0) == kCFCompareEqualTo) stats->webAreas++;
    if (CFStringCompare((CFStringRef)role, CFSTR("AXButton"), 0) == kCFCompareEqualTo) stats->buttons++;
    if (CFStringCompare((CFStringRef)role, CFSTR("AXTextField"), 0) == kCFCompareEqualTo) stats->textFields++;
    if (CFStringCompare((CFStringRef)role, CFSTR("AXStaticText"), 0) == kCFCompareEqualTo) stats->staticTexts++;
  }
  if (role) CFRelease(role);
  CFTypeRef children = NULL;
  AXError err = AXUIElementCopyAttributeValue(element, kAXChildrenAttribute, &children);
  if (err == kAXErrorSuccess && children && CFGetTypeID(children) == CFArrayGetTypeID()) {
    CFArrayRef array = (CFArrayRef)children;
    for (CFIndex i = 0; i < CFArrayGetCount(array) && stats->visited < 400; i++) {
      CFTypeRef child = CFArrayGetValueAtIndex(array, i);
      if (child && CFGetTypeID(child) == AXUIElementGetTypeID())
        walk((AXUIElementRef)child, depth - 1, stats);
    }
  } else if (err != kAXErrorNoValue && err != kAXErrorAttributeUnsupported) {
    stats->childErrors++;
  }
  if (children) CFRelease(children);
}

static void sample(AXUIElementRef app, const char *stage) {
  Stats stats = {0};
  walk(app, 10, &stats);
  printf("%s visited=%d web_areas=%d buttons=%d text_fields=%d static_texts=%d child_errors=%d\n",
         stage, stats.visited, stats.webAreas, stats.buttons, stats.textFields,
         stats.staticTexts, stats.childErrors);
  fflush(stdout);
}

int main(int argc, char **argv) {
  if (argc != 3 || (strcmp(argv[2], "--check") && strcmp(argv[2], "--enable-manual"))) {
    fputs("usage: AXEnableProbe SELECTED_DISPOSABLE_APP_PID --check|--enable-manual\n", stderr);
    return 2;
  }
  char *end = NULL;
  long parsed = strtol(argv[1], &end, 10);
  if (!end || *end || parsed <= 0 || parsed > INT32_MAX) return 2;
  pid_t pid = (pid_t)parsed;
  AXUIElementRef app = AXUIElementCreateApplication(pid);
  pid_t actualPID = 0;
  AXError pidStatus = AXUIElementGetPid(app, &actualPID);
  printf("trusted=%d target_pid=%d app_pid=%d app_pid_status=%d\n",
         AXIsProcessTrusted(), pid, actualPID, pidStatus);
  if (pidStatus != kAXErrorSuccess || actualPID != pid) {
    CFRelease(app);
    return 1;
  }
  sample(app, "before");
  if (!strcmp(argv[2], "--enable-manual")) {
    AXError setStatus = AXUIElementSetAttributeValue(app, CFSTR("AXManualAccessibility"), kCFBooleanTrue);
    printf("manual_set_status=%d\n", setStatus);
    fflush(stdout);
    if (setStatus == kAXErrorSuccess) {
      for (int i = 0; i < 20; i++) {
        usleep(100000);
        if (i == 4 || i == 9 || i == 19) sample(app, i == 4 ? "after_0_5s" : i == 9 ? "after_1s" : "after_2s");
      }
    }
  }
  CFRelease(app);
  return 0;
}
