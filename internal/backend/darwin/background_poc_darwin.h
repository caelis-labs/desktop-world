// Compiled only with dtw_background_poc. Window routing is private SPI.
// Research references and acceptance limits live in poc/background-input/README.md.
#include <dlfcn.h>
#include <objc/message.h>

typedef AXError (*POCWindowID)(AXUIElementRef, CGWindowID *);
typedef void (*POCWindowLocation)(CGEventRef, double, double);
typedef void (*POCPost)(pid_t, CGEventRef);
typedef OSStatus (*POCRecordPost)(const ProcessSerialNumber *, const uint8_t *);
typedef OSStatus (*POCProcessPSN)(pid_t, ProcessSerialNumber *);
typedef void (*POCSetAuth)(CGEventRef, id);
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
// Chromium on modern macOS accepts PID-routed key events only when SkyLight
// authenticates the event record. The factory is optional (older macOS); a
// missing selector leaves the public PID route available for native controls.
static void pocAuthenticateKey(void *sky, CGEventRef event, pid_t pid) {
  POCSetAuth setAuth = sky ? (POCSetAuth)dlsym(sky, "SLEventSetAuthenticationMessage") : NULL;
  Class messageClass = NSClassFromString(@"SLSEventAuthenticationMessage");
  SEL factory = NSSelectorFromString(@"messageWithEventRecord:pid:version:");
  if (!setAuth || !messageClass || ![messageClass respondsToSelector:factory]) return;
  // SkyLight's CGEvent record slot is at 24 on the supported 64-bit host.
  // A missing record simply leaves the event unauthenticated; no second post.
  void *record = *(void **)((uint8_t *)event + 24);
  if (!record) return;
  typedef id (*Factory)(id, SEL, void *, int, unsigned);
  id message = ((Factory)objc_msgSend)(messageClass, factory, record, pid, 0);
  if (message) setAuth(event, message);
}
static void pocStampWindowEvent(CGEventRef event, pid_t pid, CGWindowID wid,
                                POCWindowLocation setLocation, CGRect rect,
                                CGPoint point) {
  CGEventSetFlags(event, 0);
  CGEventSetLocation(event, point);
  CGEventSetIntegerValueField(event, (CGEventField)7, 3);
  CGEventSetIntegerValueField(event, (CGEventField)40, pid);
  CGEventSetIntegerValueField(event, (CGEventField)51, wid);
  CGEventSetIntegerValueField(event, (CGEventField)91, wid);
  CGEventSetIntegerValueField(event, (CGEventField)92, wid);
  setLocation(event, point.x - rect.origin.x, point.y - rect.origin.y);
}
static NSDictionary *backgroundPOC(DWContext *c, NSDictionary *request, DWCancel *cancel) {
  NSDictionary *o = request[@"Operation"], *s = o[@"Step"];
  NSString *key = o[@"Key"], *op = s[@"Op"], *mode = request[@"Mode"];
  if (cancelled(cancel)) return outcome(@"none", @"cancelled");
  if (!AXIsProcessTrusted()) return outcome(@"none", @"permission_denied");
  AXUIElementRef e = element(c, key);
  if (!e || !alive(c, key)) return outcome(@"none", @"ref_gone");
  NSDictionary *meta = c.meta[key.integerValue - 1];
  NSString *windowKey = [attr(e, kAXRoleAttribute) isEqual:@"AXWindow"] ? key : meta[@"Window"];
  AXUIElementRef window = element(c, windowKey);
  if (!window || !alive(c, windowKey)) return outcome(@"none", @"background_unavailable");
  pid_t pid = [meta[@"PID"] intValue];
  if (![mode isEqual:@"public_pid"] && ![mode isEqual:@"skylight"] && ![mode isEqual:@"no_raise"]) return outcome(@"none", @"background_unavailable");
  void *sky = dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight", RTLD_LAZY);
  POCWindowID getWindow = (POCWindowID)dlsym(RTLD_DEFAULT, "_AXUIElementGetWindow");
  POCWindowLocation setLocation = sky ? (POCWindowLocation)dlsym(sky, "CGEventSetWindowLocation") : NULL;
  POCPost post = [mode isEqual:@"skylight"] && sky ? (POCPost)dlsym(sky, "SLEventPostToPid") : NULL;
  if (!post) post = CGEventPostToPid;
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
    if (sky) dlclose(sky);
    return outcome(@"none", @"background_unavailable");
  }
  BOOL move = [op isEqual:@"pointer.move"];
  BOOL click = [op isEqual:@"pointer.click"];
  BOOL scroll = [op isEqual:@"pointer.scroll"];
  BOOL press = [op isEqual:@"keyboard.press"];
  BOOL type = [op isEqual:@"keyboard.type_text"];
  NSString *text = type ? s[@"TypeText"][@"Text"] : nil;
  // Custom image/canvas views in the owned AppKit probe drop PID-routed
  // clicks. Decide before posting; an ignored click cannot be replayed.
  if ((click || move) && [attr(e, kAXRoleAttribute) isEqual:(__bridge NSString *)kAXImageRole]) {
    if (sky) dlclose(sky);
    return outcome(@"none", @"background_unavailable");
  }
  if ((!move && !click && !scroll && !press && !type) ||
      (type && (text.length > 256 || [text rangeOfCharacterFromSet:NSCharacterSet.controlCharacterSet].location != NSNotFound))) {
    if (sky) dlclose(sky);
    return outcome(@"none", @"background_unavailable");
  }
  if (!CGPreflightPostEventAccess()) {
    if (sky) dlclose(sky);
    return outcome(@"none", @"permission_denied");
  }
  AXUIElementRef app = AXUIElementCreateApplication(pid);
  if (type || press) {
    NSString *role = attr(e, kAXRoleAttribute);
    if (type && (![role isEqual:(__bridge NSString *)kAXTextFieldRole] && ![role isEqual:(__bridge NSString *)kAXTextAreaRole]) ||
        [attr(e, kAXSubroleAttribute) isEqual:@"AXSecureTextField"]) {
      CFRelease(app);
      if (sky) dlclose(sky);
      return outcome(@"none", @"background_unavailable");
    }
    id focusedWindow = attr(app, kAXFocusedWindowAttribute), focused = attr(app, kAXFocusedUIElementAttribute);
    BOOL exactElement = focused && CFEqual((__bridge CFTypeRef)focused, e);
    BOOL exactWindow = focusedWindow && CFEqual((__bridge CFTypeRef)focusedWindow, window);
#ifdef DTW_VIRTUAL_INPUT_POC
    // An AppKit sheet can be the focused AXWindow while discovery still
    // associates its text child with the main window. Exact focused-element
    // identity plus a live same-PID sheet is sufficient for PID keyboard
    // delivery; no foreground activation or re-post is needed.
    if (!exactWindow && exactElement && focusedWindow) {
      CGWindowID focusedWID = 0;
      pid_t focusedPID = 0;
      AXUIElementGetPid((__bridge AXUIElementRef)focusedWindow, &focusedPID);
      if (focusedPID == pid && getWindow((__bridge AXUIElementRef)focusedWindow, &focusedWID) == kAXErrorSuccess && focusedWID) {
        for (NSDictionary *candidate in info) {
          if ([candidate[(id)kCGWindowNumber] unsignedIntValue] == focusedWID &&
              [candidate[(id)kCGWindowOwnerPID] intValue] == pid) {
            exactWindow = YES;
            break;
          }
        }
      }
    }
#endif
    if (!exactWindow || (type && !exactElement) ||
        (press && ![role isEqual:@"AXWindow"] && !exactElement)) {
      CFRelease(app);
      if (sky) dlclose(sky);
      return outcome(@"none", @"background_target_not_focused");
    }
  }
  CFRelease(app);
  double start = monotonicSeconds();
  NSString *delivery = @"complete", *failure = nil;
  double leaseMS = 0;
  NSDictionary *p = [o[@"Point"] isKindOfClass:NSDictionary.class] ? o[@"Point"] : nil;
  CGPoint point = CGPointMake([p[@"X"] doubleValue], [p[@"Y"] doubleValue]);
  if ((move || click || scroll) && (!p || !CGRectContainsPoint(rect, point))) {
    if (sky) dlclose(sky);
    return outcome(@"none", @"point_out_of_bounds");
  }
  if (move || click || scroll) {
    CGEventRef primer = CGEventCreateMouseEvent(NULL, kCGEventMouseMoved, point, kCGMouseButtonLeft);
    CGEventRef wheel = scroll ? CGEventCreateScrollWheelEvent(NULL, kCGScrollEventUnitLine, 2,
        -[s[@"Scroll"][@"DY"] intValue], -[s[@"Scroll"][@"DX"] intValue]) : NULL;
    NSString *buttonName = click ? s[@"Click"][@"Button"] : @"left";
    CGMouseButton button = [buttonName isEqual:@"right"] ? kCGMouseButtonRight :
                           [buttonName isEqual:@"middle"] ? kCGMouseButtonCenter : kCGMouseButtonLeft;
    CGEventType down = button == kCGMouseButtonRight ? kCGEventRightMouseDown :
                       button == kCGMouseButtonCenter ? kCGEventOtherMouseDown : kCGEventLeftMouseDown;
    CGEventType up = button == kCGMouseButtonRight ? kCGEventRightMouseUp :
                     button == kCGMouseButtonCenter ? kCGEventOtherMouseUp : kCGEventLeftMouseUp;
    int pairs = click ? [s[@"Click"][@"Count"] intValue] : 0;
    CGEventRef clicks[4] = {0};
    BOOL allocated = primer && (!scroll || wheel) && pairs >= 0 && pairs <= 2;
    for (int i = 0; allocated && i < pairs; i++) {
      clicks[2*i] = CGEventCreateMouseEvent(NULL, down, point, button);
      clicks[2*i+1] = CGEventCreateMouseEvent(NULL, up, point, button);
      allocated = clicks[2*i] && clicks[2*i+1];
    }
    if (!allocated) {
      if (primer) CFRelease(primer);
      if (wheel) CFRelease(wheel);
      for (int i = 0; i < 4; i++) if (clicks[i]) CFRelease(clicks[i]);
      if (sky) dlclose(sky);
      return outcome(@"none", @"input_rejected");
    }
    pocStampWindowEvent(primer, pid, wid, setLocation, rect, point);
    if (wheel) pocStampWindowEvent(wheel, pid, wid, setLocation, rect, point);
    for (int i = 0; i < pairs; i++) {
      for (int j = 0; j < 2; j++) {
        CGEventRef event = clicks[2*i+j];
        pocStampWindowEvent(event, pid, wid, setLocation, rect, point);
        CGEventSetIntegerValueField(event, kCGMouseEventClickState, i+1);
      }
    }
    // Post once to the retained PID/window. A posted-but-unverified gesture
    // must never trigger an automatic second route.
    post(pid, primer);
#ifdef DTW_VIRTUAL_INPUT_POC
    fprintf(stderr, "{\"poc\":\"virtual_pointer\",\"x\":%.3f,\"y\":%.3f}\n", point.x, point.y);
#endif
    if (wheel) {
      usleep(12000);
      post(pid, wheel);
    }
    for (int i = 0; i < pairs; i++) {
      usleep(12000);
      post(pid, clicks[2*i]);
      usleep(12000);
      post(pid, clicks[2*i+1]);
    }
    CFRelease(primer);
    if (wheel) CFRelease(wheel);
    for (int i = 0; i < 4; i++) if (clicks[i]) CFRelease(clicks[i]);
  } else if (press) {
    CGKeyCode code = keycode(s[@"Press"][@"Key"]);
    if (code == UINT16_MAX) { if (sky) dlclose(sky); return outcome(@"none", @"background_unavailable"); }
    NSDictionary *masks = @{@"primary": @(kCGEventFlagMaskCommand), @"meta": @(kCGEventFlagMaskCommand),
        @"control": @(kCGEventFlagMaskControl), @"alt": @(kCGEventFlagMaskAlternate),
        @"shift": @(kCGEventFlagMaskShift)};
    CGEventFlags flags = 0;
    NSArray *modifiers = [s[@"Press"][@"Modifiers"] isKindOfClass:NSArray.class] ? s[@"Press"][@"Modifiers"] : @[];
    for (NSString *modifier in modifiers) flags |= [masks[modifier] unsignedLongLongValue];
    CGEventRef down = CGEventCreateKeyboardEvent(NULL, code, true);
    CGEventRef up = CGEventCreateKeyboardEvent(NULL, code, false);
    if (!down || !up) {
      if (down) CFRelease(down);
      if (up) CFRelease(up);
      if (sky) dlclose(sky);
      return outcome(@"none", @"input_rejected");
    }
    CGEventSetFlags(down, flags);
    CGEventSetFlags(up, flags);
    if ([mode isEqual:@"skylight"]) {
      pocAuthenticateKey(sky, down, pid);
      pocAuthenticateKey(sky, up, pid);
    }
    post(pid, down);
    usleep(8000);
    post(pid, up);
    CFRelease(down);
    CFRelease(up);
  } else if (type) {
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
      if ([mode isEqual:@"skylight"]) {
        pocAuthenticateKey(sky, down, pid);
        pocAuthenticateKey(sky, up, pid);
      }
      post(pid, down); post(pid, up);
      CFRelease(down); CFRelease(up);
      i += count;
      usleep(3000);
    }
  }
  fprintf(stderr, "{\"poc\":\"targeted_input\",\"mode\":\"%s\",\"operation\":\"%s\",\"native_ms\":%.3f,\"focus_lease_ms\":%.3f}\n", mode.UTF8String, op.UTF8String, (monotonicSeconds()-start)*1000, leaseMS);
  if (sky) dlclose(sky);
  // Posting proves dispatch only. The caller must verify the app-owned result.
  return outcome(delivery, failure);
}
