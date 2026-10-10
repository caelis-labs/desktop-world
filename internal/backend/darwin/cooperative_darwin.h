// A bounded foreground transaction. Uses exact-window key-focus SPI and HID
// input. Automatic routing enters this path only after a proven pre-dispatch
// background refusal or for operations that require the physical foreground.
// The foreground transaction checks WindowServer directly, including
// switches made by a different process. No host NSApplication/run loop is
// required.
#include <dlfcn.h>
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
static pid_t cooperativeFrontPID(void) {
  typedef OSStatus (*FrontPSN)(ProcessSerialNumber *);
  static void *sky;
  static FrontPSN getFront;
  static dispatch_once_t once;
  dispatch_once(&once, ^{
    sky =
        dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight",
               RTLD_LAZY);
    getFront = sky ? (FrontPSN)dlsym(sky, "_SLPSGetFrontProcess") : NULL;
  });
  ProcessSerialNumber psn = {0};
  pid_t pid = 0;
  return getFront && getFront(&psn) == noErr &&
                 GetProcessPID(&psn, &pid) == noErr
             ? pid
             : 0;
}
#pragma clang diagnostic pop
static BOOL cooperativeAvailable(void) {
  void *sky =
      dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight",
             RTLD_LAZY);
  BOOL ready = sky && dlsym(sky, "_SLPSGetFrontProcess") &&
               dlsym(sky, "SLPSSetFrontProcessWithOptions") &&
               dlsym(sky, "SLPSPostEventRecordTo") &&
               dlsym(RTLD_DEFAULT, "GetProcessForPID") &&
               dlsym(RTLD_DEFAULT, "_AXUIElementGetWindow");
  if (sky)
    dlclose(sky);
  return ready;
}
static BOOL cooperativeOnScreen(pid_t pid, AXUIElementRef window) {
  typedef AXError (*WindowID)(AXUIElementRef, CGWindowID *);
  WindowID getWindow = (WindowID)dlsym(RTLD_DEFAULT, "_AXUIElementGetWindow");
  CGWindowID wid = 0;
  if (!getWindow || getWindow(window, &wid) != kAXErrorSuccess || !wid)
    return NO;
  NSArray *windows = CFBridgingRelease(CGWindowListCopyWindowInfo(
      kCGWindowListOptionOnScreenOnly, kCGNullWindowID));
  for (NSDictionary *row in windows)
    if ([row[(id)kCGWindowNumber] unsignedIntValue] == wid &&
        [row[(id)kCGWindowOwnerPID] intValue] == pid)
      return YES;
  return NO;
}
static CGPoint cooperativeCursor(void) {
  CGEventRef e = CGEventCreate(NULL);
  CGPoint p = e ? CGEventGetLocation(e) : CGPointMake(NAN, NAN);
  if (e)
    CFRelease(e);
  return p;
}
static BOOL cooperativeFocusedWindow(pid_t pid, AXUIElementRef window) {
  if (cooperativeFrontPID() != pid)
    return NO;
  AXUIElementRef app = AXUIElementCreateApplication(pid);
  id focused = attr(app, kAXFocusedWindowAttribute);
  CFRelease(app);
  return focused && CFEqual((__bridge CFTypeRef)focused, window);
}
static BOOL cooperativeActivate(pid_t pid, AXUIElementRef window,
                                double deadline) {
  if (!window || monotonicSeconds() >= deadline)
    return NO;
  typedef AXError (*WindowID)(AXUIElementRef, CGWindowID *);
  typedef OSStatus (*ProcessPSN)(pid_t, ProcessSerialNumber *);
  typedef OSStatus (*Front)(const ProcessSerialNumber *, uint32_t, uint32_t);
  typedef OSStatus (*Record)(const ProcessSerialNumber *, const uint8_t *);
  void *sky =
      dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight",
             RTLD_LAZY);
  WindowID getWindow = (WindowID)dlsym(RTLD_DEFAULT, "_AXUIElementGetWindow");
  ProcessPSN getPSN = (ProcessPSN)dlsym(RTLD_DEFAULT, "GetProcessForPID");
  Front setFront =
      sky ? (Front)dlsym(sky, "SLPSSetFrontProcessWithOptions") : NULL;
  Record post = sky ? (Record)dlsym(sky, "SLPSPostEventRecordTo") : NULL;
  CGWindowID wid = 0;
  ProcessSerialNumber psn = {0};
  BOOL valid = getWindow && getPSN && setFront && post &&
               getWindow(window, &wid) == kAXErrorSuccess && wid &&
               getPSN(pid, &psn) == noErr;
  if (!valid) {
    if (sky)
      dlclose(sky);
    return NO;
  }
  BOOL sent = setFront(&psn, wid, 0x200) == noErr;
  for (int kind = 1; sent && kind <= 2; kind++) {
    uint8_t record[0xf8] = {0};
    record[4] = 0xf8;
    record[8] = kind;
    record[0x3a] = 0x10;
    memcpy(record + 0x3c, &wid, sizeof(wid));
    memset(record + 0x20, 0xff, 0x10);
    sent = post(&psn, record) == noErr;
  }
  dlclose(sky);
  if (!sent)
    return NO;
  AXUIElementSetMessagingTimeout(window, .15);
  AXError raised = AXUIElementPerformAction(window, kAXRaiseAction);
  AXUIElementSetMessagingTimeout(window, .25);
  if (raised != kAXErrorSuccess)
    return NO;
  while (monotonicSeconds() < deadline) {
    if (cooperativeFocusedWindow(pid, window)) {
      usleep(20000);
      return YES;
    }
    usleep(10000);
  }
  return NO;
}
static NSDictionary *cooperativeEnd(DWContext *c) {
  NSMutableDictionary *session = c.inputSession;
  if (!session)
    return @{
      @"Result" : c.inputReport ?: @{
        @"Mode" : @"cooperative",
        @"ForegroundMS" : @0,
        @"Restoration" : @"not_borrowed"
      }
    };
#ifndef DTW_VIRTUAL_INPUT_POC
  c.inputSession = nil;
#endif
  BOOL released = YES;
  for (NSString *held in c.inputHeld.allKeys) {
    unsigned code = (unsigned)[[held substringFromIndex:1] intValue];
    BOOL mouse = [held hasPrefix:@"b"];
    CGEventType up = code == 0   ? kCGEventLeftMouseUp
                     : code == 1 ? kCGEventRightMouseUp
                                 : kCGEventOtherMouseUp;
    CGPoint releasePoint = cooperativeCursor();
#ifdef DTW_VIRTUAL_INPUT_POC
    if (mouse && session[@"virtualX"] && session[@"virtualY"])
      releasePoint = CGPointMake([session[@"virtualX"] doubleValue], [session[@"virtualY"] doubleValue]);
#endif
    CGEventRef event =
        mouse ? CGEventCreateMouseEvent(NULL, up, releasePoint, code)
              : CGEventCreateKeyboardEvent(NULL, code, false);
    if (!event) {
      released = NO;
      continue;
    }
    if (!mouse) {
      CGEventFlags mask = code == 55   ? kCGEventFlagMaskCommand
                          : code == 59 ? kCGEventFlagMaskControl
                          : code == 58 ? kCGEventFlagMaskAlternate
                          : code == 56 ? kCGEventFlagMaskShift
                                       : 0;
      CGEventSetFlags(event, CGEventSourceFlagsState(
                                 kCGEventSourceStateCombinedSessionState) &
                                 ~mask);
    }
#ifdef DTW_VIRTUAL_INPUT_POC
    if (!virtualPostToTarget(c, event)) released = NO;
#else
    CGEventPost(kCGHIDEventTap, event);
#endif
    CFRelease(event);
    [c.inputHeld removeObjectForKey:held];
  }
#ifdef DTW_VIRTUAL_INPUT_POC
  c.inputSession = nil;
#endif
  NSString *restoration = @"not_borrowed";
  double start = [session[@"start"] doubleValue];
  if ([session[@"borrowed"] boolValue]) {
    // A genuinely required foreground action gets a short settling window.
    // Consecutive steps in one native plan already share this lease; the
    // trailing grace avoids an immediate activate/restore flash. Stop waiting
    // as soon as the user selects another app and never take it back.
    double lastAction = [session[@"lastAction"] doubleValue];
    double graceEnd = lastAction + .45;
    while (lastAction > 0 && monotonicSeconds() < graceEnd &&
           cooperativeFrontPID() == [session[@"targetPID"] intValue] &&
           !c.inputFault)
      usleep(10000);
    pid_t pid = [session[@"previousPID"] intValue];
    if (cooperativeFrontPID() != [session[@"targetPID"] intValue])
      restoration = @"user_superseded";
    else {
      AXUIElementRef window =
          (__bridge AXUIElementRef)session[@"previousWindow"];
      BOOL identity =
          [processIdentity(pid) isEqual:session[@"previousIdentity"]];
      if (identity &&
          cooperativeActivate(pid, window, monotonicSeconds() + .5)) {
        id focused = session[@"previousFocus"];
        if (focused)
          AXUIElementSetAttributeValue((__bridge AXUIElementRef)focused,
                                       kAXFocusedAttribute, kCFBooleanTrue);
        AXUIElementRef app = AXUIElementCreateApplication(pid);
        id actual = attr(app, kAXFocusedUIElementAttribute);
        CFRelease(app);
        BOOL focusRestored =
            !focused || (actual && CFEqual((__bridge CFTypeRef)actual,
                                           (__bridge CFTypeRef)focused));
        restoration = cooperativeFocusedWindow(pid, window) && focusRestored
                          ? @"restored"
                          : @"failed";
      } else
        restoration = @"failed";
    }
  }
  // A user cursor movement wins. Only undo our own final pointer position.
  if (session[@"lastX"] && [restoration isEqual:@"restored"]) {
#ifndef DTW_VIRTUAL_INPUT_POC
    CGPoint current = cooperativeCursor();
    CGPoint last = CGPointMake([session[@"lastX"] doubleValue],
                               [session[@"lastY"] doubleValue]);
    if (CGPointEqualToPoint(current, last))
      CGWarpMouseCursorPosition(CGPointMake([session[@"x"] doubleValue],
                                            [session[@"y"] doubleValue]));
#endif
  }
  if (!released)
    restoration = @"failed";
  c.inputReport = @{
    @"Mode" : @"cooperative",
    @"ForegroundMS" :
        @([session[@"borrowed"] boolValue]
              ? (long long)ceil((monotonicSeconds() - start) * 1000)
              : 0),
    @"Restoration" : restoration
  };
  return @{@"Result" : c.inputReport};
}
static NSString *cooperativeGuard(DWContext *c) {
  if (c.inputFault)
    return c.inputFault;
  if (!c.inputSession)
    return nil;
  if (cooperativeFrontPID() != [c.inputSession[@"targetPID"] intValue])
    return @"user_interrupted";
  if (monotonicSeconds() >= [c.inputSession[@"deadline"] doubleValue]) {
    c.inputFault = @"input_lease_expired";
    cooperativeEnd(c);
    return c.inputFault;
  }
  return nil;
}
static BOOL cooperativeHitsOnce(AXUIElementRef e, NSDictionary *p) {
  AXUIElementRef sys = AXUIElementCreateSystemWide(), hit = NULL;
  BOOL found = AXUIElementCopyElementAtPosition(sys, [p[@"X"] floatValue],
                                                [p[@"Y"] floatValue],
                                                &hit) == kAXErrorSuccess;
  BOOL exact = NO;
  id current = hit ? (__bridge id)hit : nil;
  NSMutableSet *seen = [NSMutableSet set];
  for (int depth = 0; found && current && depth < 64; depth++) {
    if (CFEqual((__bridge CFTypeRef)current, e)) {
      exact = YES;
      break;
    }
    if ([seen containsObject:current])
      break;
    [seen addObject:current];
    current = attr((__bridge AXUIElementRef)current, kAXParentAttribute);
  }
  if (hit)
    CFRelease(hit);
  CFRelease(sys);
  return exact;
}
// Focus acknowledgement can precede compositor/AX hit-test convergence. Wait
// only before dispatch, for the same retained target, under the original lease.
// This never re-posts input, chooses another target, or reactivates a window.
static BOOL cooperativeHits(DWContext *c, AXUIElementRef e, NSDictionary *p) {
  double deadline =
      MIN(monotonicSeconds() + .12, [c.inputSession[@"deadline"] doubleValue]);
  do {
    NSString *fault = cooperativeGuard(c);
    if (fault) {
      c.inputFault = fault;
      return NO;
    }
    if (cooperativeHitsOnce(e, p))
      return YES;
    if (monotonicSeconds() >= deadline)
      return NO;
    usleep(10000);
  } while (YES);
}
static NSDictionary *cooperativePerform(DWContext *c, NSDictionary *o,
                                        DWCancel *cancel) {
  NSDictionary *s = o[@"Step"];
  NSString *op = s[@"Op"], *key = o[@"Key"];
  if (cancelled(cancel))
    return outcome(@"none", @"cancelled");
  if (!AXIsProcessTrusted() || !CGPreflightPostEventAccess())
    return outcome(@"none", @"permission_denied");
  AXUIElementRef e = element(c, key);
  if (!e || !alive(c, key))
    return outcome(@"none", @"ref_gone");
  NSDictionary *meta = c.meta[key.integerValue - 1];
  NSString *windowKey =
      [attr(e, kAXRoleAttribute) isEqual:@"AXWindow"] ? key : meta[@"Window"];
  AXUIElementRef window = element(c, windowKey);
  pid_t pid = [meta[@"PID"] intValue];
#ifdef DTW_VIRTUAL_INPUT_POC
  id focusedWindow = nil;
  if ([op hasPrefix:@"keyboard."]) {
    AXUIElementRef targetApp = AXUIElementCreateApplication(pid);
    id focusedElement = attr(targetApp, kAXFocusedUIElementAttribute);
    focusedWindow = attr(targetApp, kAXFocusedWindowAttribute);
    CFRelease(targetApp);
    pid_t focusedPID = 0;
    if (focusedElement && focusedWindow &&
        CFEqual((__bridge CFTypeRef)focusedElement, e) &&
        AXUIElementGetPid((__bridge AXUIElementRef)focusedWindow, &focusedPID) == kAXErrorSuccess &&
        focusedPID == pid && cooperativeOnScreen(pid, (__bridge AXUIElementRef)focusedWindow))
      window = (__bridge AXUIElementRef)focusedWindow;
  }
#endif
  if (!window || !alive(c, windowKey) || !pid)
    return outcome(@"none", @"background_unavailable");
  if (!cooperativeAvailable() || !cooperativeOnScreen(pid, window))
    return outcome(@"none", @"background_unavailable");
  // Bound each burst before activation. No long drag or unbounded text owns the
  // desktop while the agent reasons. Split a known task into fresh
  // transactions.
  if ([op isEqual:@"pointer.drag"] &&
      [s[@"Drag"][@"Duration"] longLongValue] > 500000000)
    return outcome(@"none", @"input_burst_limit");
  if ([op isEqual:@"keyboard.type_text"] &&
      [s[@"TypeText"][@"Text"] length] > 256)
    return outcome(@"none", @"input_burst_limit");
  if (CGEventSourceFlagsState(kCGEventSourceStateCombinedSessionState) &
      (kCGEventFlagMaskShift | kCGEventFlagMaskControl |
       kCGEventFlagMaskAlternate | kCGEventFlagMaskCommand))
    return outcome(@"none", @"user_interrupted");
  for (int b = 0; b < 3; b++)
    if (CGEventSourceButtonState(kCGEventSourceStateCombinedSessionState, b))
      return outcome(@"none", @"user_interrupted");
  NSString *guard = cooperativeGuard(c);
  if (guard)
    return outcome(@"none", guard);
  if (!c.inputSession) {
    c.inputReport = nil;
    c.inputHeld = [NSMutableDictionary dictionary];
    pid_t previous = cooperativeFrontPID();
    if (previous <= 0)
      return outcome(@"none", @"background_unavailable");
    AXUIElementRef app = AXUIElementCreateApplication(previous);
    id previousWindow = attr(app, kAXFocusedWindowAttribute),
       previousFocus = attr(app, kAXFocusedUIElementAttribute);
    CFRelease(app);
    if (!previousWindow)
      return outcome(@"none", @"background_unavailable");
#ifdef DTW_VIRTUAL_INPUT_POC
    typedef AXError (*WindowID)(AXUIElementRef, CGWindowID *);
    WindowID getWindow = (WindowID)dlsym(RTLD_DEFAULT, "_AXUIElementGetWindow");
    CGWindowID nativeWindowID = 0;
    if (!getWindow || getWindow(window, &nativeWindowID) != kAXErrorSuccess || !nativeWindowID)
      return outcome(@"none", @"background_unavailable");
#endif
    CGPoint cursor = cooperativeCursor();
    if (!isfinite(cursor.x) || !isfinite(cursor.y))
      return outcome(@"none", @"background_unavailable");
    double start = monotonicSeconds();
    BOOL borrowed =
        previous != pid || !CFEqual((__bridge CFTypeRef)previousWindow, window);
    c.inputSession = [@{
      @"previousPID" : @(previous),
      @"previousWindow" : previousWindow,
      @"previousIdentity" : processIdentity(previous) ?: @[],
      @"targetPID" : @(pid),
      @"start" : @(start),
      @"deadline" : @(start + 1.0),
      @"borrowed" : @(borrowed),
#ifdef DTW_VIRTUAL_INPUT_POC
      @"targetWindowID" : @(nativeWindowID),
#endif
      @"x" : @(cursor.x),
      @"y" : @(cursor.y)
    } mutableCopy];
    if (previousFocus)
      c.inputSession[@"previousFocus"] = previousFocus;
    if (!cooperativeActivate(pid, window, start + .4))
      return outcome(@"none", @"needs_user_focus");
  } else {
    // A transaction never changes application. A newly bound dialog in the
    // same application may be selected using its exact live window identity.
    if ([c.inputSession[@"targetPID"] intValue] != pid)
      return outcome(@"none", @"input_transaction_scope");
#ifdef DTW_VIRTUAL_INPUT_POC
    typedef AXError (*WindowID)(AXUIElementRef, CGWindowID *);
    WindowID getWindow = (WindowID)dlsym(RTLD_DEFAULT, "_AXUIElementGetWindow");
    CGWindowID nativeWindowID = 0;
    if (!getWindow || getWindow(window, &nativeWindowID) != kAXErrorSuccess || !nativeWindowID)
      return outcome(@"none", @"background_unavailable");
    c.inputSession[@"targetWindowID"] = @(nativeWindowID);
#endif
    if (!cooperativeFocusedWindow(pid, window) &&
        !cooperativeActivate(pid, window,
                             MIN(monotonicSeconds() + .2,
                                 [c.inputSession[@"deadline"] doubleValue])))
      return outcome(@"none", @"needs_user_focus");
  }
  if (cancelled(cancel))
    return outcome(@"none", @"cancelled");
  if (!cooperativeFocusedWindow(pid, window))
    return outcome(@"none", @"user_interrupted");
  if ([op hasPrefix:@"keyboard."] || [op isEqual:@"focus"]) {
    AXUIElementRef app = AXUIElementCreateApplication(pid);
    if (!CFEqual(e, window))
      AXUIElementSetAttributeValue(e, kAXFocusedAttribute, kCFBooleanTrue);
    id focus = attr(app, kAXFocusedUIElementAttribute);
    CFRelease(app);
    if (!focus ||
        (!CFEqual(e, window) && !CFEqual((__bridge CFTypeRef)focus, e)))
      return outcome(@"none", @"background_target_not_focused");
    if ([attr((__bridge AXUIElementRef)focus, kAXSubroleAttribute)
            isEqual:@"AXSecureTextField"])
      return outcome(@"none", @"protected_target");
    if ([op isEqual:@"focus"]) {
      c.inputSession[@"lastAction"] = @(monotonicSeconds());
      return outcome(@"complete", nil);
    }
  }
  if ([op hasPrefix:@"pointer."]) {
    if (!cooperativeHits(c, e, o[@"Point"]))
      return outcome(@"none", c.inputFault ?: @"target_not_hittable");
    if ([op isEqual:@"pointer.drag"]) {
      AXUIElementRef destination = element(c, o[@"ToKey"]);
      if (!destination || !alive(c, o[@"ToKey"]) ||
          !cooperativeHits(c, destination, o[@"To"]))
        return outcome(@"none", c.inputFault ?: @"target_not_hittable");
    }
  }
  NSDictionary *result;
  inputPostingContext = (__bridge void *)c;
  @try {
    result = perform(c, o, cancel);
  } @finally {
    inputPostingContext = NULL;
  }
  if (c.inputFault)
    result = outcome(result[@"Result"][@"Delivery"], c.inputFault);
  if (![result[@"Result"][@"Delivery"] isEqual:@"none"])
    c.inputSession[@"lastAction"] = @(monotonicSeconds());
  // Allow the target to consume posted input before restoring the front
  // process.
  usleep([op isEqual:@"pointer.click"] &&
                 [s[@"Click"][@"Button"] isEqual:@"right"]
             ? 120000
             : 40000);
  if ([op hasPrefix:@"pointer."]) {
#ifndef DTW_VIRTUAL_INPUT_POC
    CGPoint p = cooperativeCursor();
    c.inputSession[@"lastX"] = @(p.x);
    c.inputSession[@"lastY"] = @(p.y);
#endif
  }
  return result;
}
