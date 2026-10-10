#import <AppKit/AppKit.h>

int dtwPOCFrontmostPID(void) {
  NSRunningApplication *frontmost = [NSWorkspace sharedWorkspace].frontmostApplication;
  return frontmost ? (int)frontmost.processIdentifier : 0;
}
