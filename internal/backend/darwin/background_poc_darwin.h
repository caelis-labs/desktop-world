// Compiled only with dtw_background_poc. Window routing is private SPI.
// Research references and acceptance limits live in poc/background-input/README.md.
#include <dlfcn.h>

typedef AXError (*POCWindowID)(AXUIElementRef, CGWindowID *);
typedef void (*POCWindowLocation)(CGEventRef, double, double);
typedef void (*POCPost)(pid_t, CGEventRef);
typedef OSStatus (*POCRecordPost)(const ProcessSerialNumber *, const uint8_t *);
typedef OSStatus (*POCProcessPSN)(pid_t, ProcessSerialNumber *);
typedef struct {
  POCRecordPost post;
  ProcessSerialNumber previous, target;
  pid_t previousPID, targetPID;
  CGWindowID previousWindow, targetWindow;
  double start;
} POCLease;
static BOOL pocFocusRecord(POCRecordPost post, const ProcessSerialNumber *psn, CGWindowID wid, BOOL focus) {
  uint8_t record[0xf8] = {0};
  record[4] = 0xf8; record[8] = 0x0d;
  memcpy(record+0x3c, &wid, sizeof(wid));
  record[0x8a] = focus ? 1 : 2;
  return post(psn, record) == noErr;
}
static CGWindowID pocFocusedWindow(pid_t pid, POCWindowID getWindow) {
  AXUIElementRef app = AXUIElementCreateApplication(pid);
  id window = attr(app, kAXFocusedWindowAttribute);
  CGWindowID wid = 0;
  if (window) getWindow((__bridge AXUIElementRef)window, &wid);
  CFRelease(app);
  return wid;
}
// Brief key-focus lease, without changing front process or raising the window.
// It is never held while the agent reasons or waits for application verification.
static BOOL pocLeaseBegin(void *sky, POCWindowID getWindow, pid_t pid, CGWindowID wid, POCLease *lease) {
  lease->post = (POCRecordPost)dlsym(sky, "SLPSPostEventRecordTo");
  POCProcessPSN getPSN = (POCProcessPSN)dlsym(RTLD_DEFAULT, "GetProcessForPID");
  lease->previousPID = frontmostPID(); lease->targetPID = pid;
  lease->previousWindow = pocFocusedWindow(lease->previousPID, getWindow); lease->targetWindow = wid;
  if (!lease->post || !getPSN || !lease->previousPID || !lease->previousWindow || lease->previousPID == pid ||
      getPSN(lease->previousPID, &lease->previous) != noErr || getPSN(pid, &lease->target) != noErr) return NO;
  lease->start = monotonicSeconds();
  if (!pocFocusRecord(lease->post, &lease->previous, wid, NO)) return NO;
  if (!pocFocusRecord(lease->post, &lease->target, wid, YES)) {
    pocFocusRecord(lease->post, &lease->previous, lease->previousWindow, YES);
    return NO;
  }
  usleep(30000);
  return YES;
}
static BOOL pocLeaseEnd(POCWindowID getWindow, POCLease *lease) {
  // A user switch supersedes our saved focus. Do not force the old application
  // back. Unresolved focus returns unknown/fenced for host reconciliation.
  if (frontmostPID() != lease->previousPID) return NO;
  CGWindowID current = pocFocusedWindow(lease->previousPID, getWindow);
  if (!current) return NO;
  BOOL defocused = pocFocusRecord(lease->post, &lease->target, lease->targetWindow, NO);
  BOOL restored = pocFocusRecord(lease->post, &lease->previous, current, YES);
  usleep(10000);
  return defocused && restored;
}
static NSDictionary *backgroundPOC(DWContext *c, NSDictionary *request, DWCancel *cancel) {
  NSDictionary *o = request[@"Operation"], *s = o[@"Step"];
  NSString *key = o[@"Key"], *op = s[@"Op"], *mode = request[@"Mode"];
  if (cancelled(cancel)) return outcome(@"none", @"cancelled");
  if (!AXIsProcessTrusted()) return outcome(@"none", @"permission_denied");
  AXUIElementRef e = element(c, key);
  if (!e || !alive(c, key)) return outcome(@"none", @"ref_gone");
  NSDictionary *meta = c.meta[key.integerValue - 1];
  NSString *windowKey = meta[@"Window"];
  AXUIElementRef window = element(c, windowKey);
  if (!window || !alive(c, windowKey)) return outcome(@"none", @"background_unavailable");
  pid_t pid = [meta[@"PID"] intValue];
  if (![mode isEqual:@"public_pid"] && ![mode isEqual:@"skylight"] && ![mode isEqual:@"no_raise"]) return outcome(@"none", @"background_unavailable");
  void *sky = dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight", RTLD_LAZY);
  POCWindowID getWindow = (POCWindowID)dlsym(RTLD_DEFAULT, "_AXUIElementGetWindow");
  POCWindowLocation setLocation = sky ? (POCWindowLocation)dlsym(sky, "CGEventSetWindowLocation") : NULL;
  POCPost post = [mode isEqual:@"skylight"] && sky ? (POCPost)dlsym(sky, "SLEventPostToPid") : CGEventPostToPid;
  CGWindowID wid = 0;
  if (!getWindow || !setLocation || !post || getWindow(window, &wid) != kAXErrorSuccess || !wid) {
    if (sky) dlclose(sky);
    return outcome(@"none", @"background_unavailable");
  }
  // Match the retained AX window to WindowServer ownership and current desktop.
  NSArray *info = CFBridgingRelease(CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly, kCGNullWindowID));
  NSDictionary *match = nil;
  for (NSDictionary *candidate in info)
    if ([candidate[(id)kCGWindowNumber] unsignedIntValue] == wid && [candidate[(id)kCGWindowOwnerPID] intValue] == pid) { match = candidate; break; }
  CGRect rect = CGRectZero;
  if (!match || !CGRectMakeWithDictionaryRepresentation((__bridge CFDictionaryRef)match[(id)kCGWindowBounds], &rect)) {
    dlclose(sky); return outcome(@"none", @"background_unavailable");
  }
  BOOL click = [op isEqual:@"pointer.click"];
  BOOL type = [op isEqual:@"keyboard.type_text"];
  NSString *text = type ? s[@"TypeText"][@"Text"] : nil;
  if ((!click && !type) || (click && (![s[@"Click"][@"Button"] isEqual:@"left"] || [s[@"Click"][@"Count"] intValue] != 1)) || (type && (text.length > 256 || [text rangeOfCharacterFromSet:NSCharacterSet.controlCharacterSet].location != NSNotFound))) {
    dlclose(sky); return outcome(@"none", @"background_unavailable");
  }
  AXUIElementRef app = AXUIElementCreateApplication(pid);
  if (type) {
    NSString *role = attr(e, kAXRoleAttribute);
    if ((![role isEqual:(__bridge NSString *)kAXTextFieldRole] && ![role isEqual:(__bridge NSString *)kAXTextAreaRole]) ||
        [attr(e, kAXSubroleAttribute) isEqual:@"AXSecureTextField"]) {
      CFRelease(app); dlclose(sky); return outcome(@"none", @"background_unavailable");
    }
    id focusedWindow = attr(app, kAXFocusedWindowAttribute), focused = attr(app, kAXFocusedUIElementAttribute);
    if (!focusedWindow || !focused || !CFEqual((__bridge CFTypeRef)focusedWindow, window) || !CFEqual((__bridge CFTypeRef)focused, e)) {
      CFRelease(app); dlclose(sky); return outcome(@"none", @"background_target_not_focused");
    }
  }
  CFRelease(app);
  double start = monotonicSeconds();
  NSString *delivery = @"complete", *failure = nil;
  double leaseMS = 0;
  if (click) {
    NSDictionary *p = o[@"Point"];
    CGPoint point = CGPointMake([p[@"X"] doubleValue], [p[@"Y"] doubleValue]);
    if (!CGRectContainsPoint(rect, point)) { dlclose(sky); return outcome(@"none", @"point_out_of_bounds"); }
    CGEventType types[] = {kCGEventMouseMoved, kCGEventLeftMouseDown, kCGEventLeftMouseUp};
    CGEventRef events[3] = {0};
    for (int i = 0; i < 3; i++) {
      events[i] = CGEventCreateMouseEvent(NULL, types[i], point, kCGMouseButtonLeft);
      if (!events[i]) {
        for (int j = 0; j < i; j++) CFRelease(events[j]);
        dlclose(sky); return outcome(@"none", @"input_rejected");
      }
      CGEventSetFlags(events[i], 0);
      CGEventSetIntegerValueField(events[i], kCGMouseEventClickState, i ? 1 : 0);
      CGEventSetIntegerValueField(events[i], (CGEventField)7, 3);
      CGEventSetIntegerValueField(events[i], (CGEventField)40, pid);
      CGEventSetIntegerValueField(events[i], (CGEventField)51, wid);
      CGEventSetIntegerValueField(events[i], (CGEventField)91, wid);
      CGEventSetIntegerValueField(events[i], (CGEventField)92, wid);
      setLocation(events[i], point.x - rect.origin.x, point.y - rect.origin.y);
    }
    POCLease lease = {0};
    BOOL leased = [mode isEqual:@"no_raise"];
    if (leased && !pocLeaseBegin(sky, getWindow, pid, wid, &lease)) {
      for (int i = 0; i < 3; i++) CFRelease(events[i]);
      dlclose(sky); return outcome(lease.start ? @"unknown" : @"none", @"background_focus_unresolved");
    }
    // Allocate the complete pair before dispatch; no second transport/replay.
    post(pid, events[0]); usleep(12000);
    post(pid, events[1]); usleep(12000);
    post(pid, events[2]);
    for (int i = 0; i < 3; i++) CFRelease(events[i]);
    if (leased) {
      usleep(20000); // Let the target process consume mouse-up before release.
      BOOL restored = pocLeaseEnd(getWindow, &lease);
      leaseMS = (monotonicSeconds()-lease.start)*1000;
      if (!restored) { delivery = @"unknown"; failure = @"background_focus_unresolved"; }
    }
  } else {
    for (NSUInteger i = 0; i < text.length;) {
      if (cancelled(cancel)) { delivery = i ? @"partial" : @"none"; failure = @"cancelled"; break; }
      NSUInteger count = CFStringIsSurrogateHighCharacter([text characterAtIndex:i]) && i+1 < text.length ? 2 : 1;
      UniChar chars[2]; [text getCharacters:chars range:NSMakeRange(i, count)];
      CGEventRef down = CGEventCreateKeyboardEvent(NULL, 0, true), up = CGEventCreateKeyboardEvent(NULL, 0, false);
      if (!down || !up) {
        if (down) CFRelease(down); if (up) CFRelease(up);
        delivery = i ? @"partial" : @"none"; failure = @"input_rejected"; break;
      }
      CGEventSetFlags(down, 0); CGEventSetFlags(up, 0);
      CGEventKeyboardSetUnicodeString(down, count, chars); CGEventKeyboardSetUnicodeString(up, count, chars);
      post(pid, down); post(pid, up);
      CFRelease(down); CFRelease(up);
      i += count;
      usleep(3000);
    }
  }
  fprintf(stderr, "{\"poc\":\"targeted_input\",\"mode\":\"%s\",\"operation\":\"%s\",\"native_ms\":%.3f,\"focus_lease_ms\":%.3f}\n", mode.UTF8String, op.UTF8String, (monotonicSeconds()-start)*1000, leaseMS);
  dlclose(sky);
  // Posting proves dispatch only. The caller must verify the app-owned result.
  return outcome(delivery, failure);
}
