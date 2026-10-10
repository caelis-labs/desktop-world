// Public ScreenCaptureKit identities are intentionally separate from AX windows.
// No title/bounds join and no desktop-crop fallback are used.
static BOOL waitNative(dispatch_semaphore_t sem, DWCancel *cancel, double deadline) {
  while (!cancelled(cancel) && monotonicSeconds() < deadline) {
    if (dispatch_semaphore_wait(sem, dispatch_time(DISPATCH_TIME_NOW, 20 * NSEC_PER_MSEC)) == 0) return YES;
  }
  return NO;
}
static SCShareableContent *shareable(DWCancel *cancel, double deadline) {
  __block SCShareableContent *content = nil;
  dispatch_semaphore_t sem = dispatch_semaphore_create(0);
  [SCShareableContent getShareableContentExcludingDesktopWindows:YES onScreenWindowsOnly:NO
    completionHandler:^(SCShareableContent *v, NSError *e) {
      if (!e) content = v;
      dispatch_semaphore_signal(sem);
    }];
  return waitNative(sem, cancel, deadline) ? content : nil;
}
static NSDictionary *captureNode(DWContext *c, NSString *k, NSArray *fields) {
  DWCaptureWindow *r = c.captureWindows[k];
  if (!r || !alive(c, k)) return nil;
  NSMutableDictionary *o = [@{@"Kind": @"window", @"Role": @"capture_window", @"Lifecycle": @"live"} mutableCopy];
  if (wantField(fields, @"name")) o[@"Name"] = r.window.title ? known(r.window.title) : unknown();
  if (wantField(fields, @"states")) o[@"States"] = @{@"visible": known(@(r.window.onScreen))};
  if (wantField(fields, @"capabilities")) o[@"Capabilities"] = @[@{@"Name": @"window_content", @"Support": @"supported", @"Availability": r.window.onScreen ? @"available" : @"blocked"}];
  CGRect b = r.window.frame;
  if (wantField(fields, @"bounds")) o[@"Bounds"] = known(@{@"Frame": @"window", @"Topology": @(c.topology), @"Rect": @{@"X": @0, @"Y": @0, @"Width": @(b.size.width), @"Height": @(b.size.height)}});
  return @{@"Fields": fields ?: @[], @"Key": k, @"App": r.app, @"Parent": r.app, @"Window": k, @"Object": o};
}
static NSDictionary *windowInfo(CGWindowID wid, BOOL *available) {
  NSArray *list = CFBridgingRelease(CGWindowListCopyWindowInfo(kCGWindowListOptionAll, kCGNullWindowID));
  *available = list != nil && list.count > 0;
  for (NSDictionary *row in list) if ([row[(id)kCGWindowNumber] unsignedIntValue] == wid) return row;
  return nil;
}
static NSString *refreshCaptureRecord(DWContext *c, DWCaptureWindow *r, SCShareableContent *content) {
  if (!r) return @"capability_unavailable";
  if (r.gone || ![r.identity isEqual:processIdentity(r.pid)]) return @"ref_gone";
  BOOL available = NO;
  NSDictionary *info = windowInfo(r.window.windowID, &available);
  if (!available) return @"seat_unavailable";
  if (!info || [info[(id)kCGWindowOwnerPID] intValue] != r.pid) { r.gone = YES; return @"ref_gone"; }
  if (![info[(id)kCGWindowIsOnscreen] boolValue]) return @"window_not_visible";
  for (SCWindow *w in content.windows) {
    if (w.windowID == r.window.windowID && w.owningApplication.processID == r.pid) {
      if (!w.onScreen) return @"window_not_visible";
      r.window = w;
      return nil;
    }
  }
  return @"window_unavailable";
}
static NSString *refreshCaptureWindow(DWContext *c, NSString *k, SCShareableContent *content) {
  return refreshCaptureRecord(c, c.captureWindows[k], content);
}
static NSDictionary *captureWindowsQuery(DWContext *c, NSDictionary *q, DWCancel *cancel) {
  if (!CGPreflightScreenCaptureAccess()) return err(@"permission_denied");
  NSMutableSet *pids = [NSMutableSet set];
  NSMutableSet *only = [NSMutableSet set];
  NSMutableSet *wholeApps = [NSMutableSet set];
  NSArray *roots = [q[@"Roots"] isKindOfClass:NSArray.class] ? q[@"Roots"] : @[];
  for (NSString *k in roots) {
    if (!alive(c, k)) return err(@"ref_gone");
    DWCaptureWindow *cw = c.captureWindows[k];
    if (cw) { [pids addObject:@(cw.pid)]; [only addObject:@(cw.window.windowID)]; continue; }
    AXUIElementRef e = element(c, k);
    if (![attr(e, kAXRoleAttribute) isEqual:@"AXApplication"]) return err(@"capture_identity_unavailable");
    NSNumber *pid = c.meta[k.integerValue - 1][@"PID"];
    [pids addObject:pid]; [wholeApps addObject:pid];
  }
  SCShareableContent *content = shareable(cancel, queryDeadline);
  if (!content) return err(cancelled(cancel) ? @"cancelled" : @"provider_unavailable");
  NSMutableArray *nodes = [NSMutableArray array];
  NSMutableDictionary *liveWindows = [NSMutableDictionary dictionary];
  NSArray *windowList = CFBridgingRelease(CGWindowListCopyWindowInfo(kCGWindowListOptionAll, kCGNullWindowID));
  if (!windowList.count) return err(@"seat_unavailable");
  for (NSDictionary *row in windowList) liveWindows[row[(id)kCGWindowNumber]] = row;
  NSInteger visited = 0, maxNodes = [q[@"MaxNodes"] integerValue];
  BOOL complete = YES;
  for (SCWindow *w in content.windows) {
    pid_t pid = w.owningApplication.processID;
    if (![q[@"Desktop"] boolValue] && (![pids containsObject:@(pid)] || (![wholeApps containsObject:@(pid)] && only.count && ![only containsObject:@(w.windowID)]))) continue;
    NSDictionary *info = liveWindows[@(w.windowID)];
    if (!w.onScreen || !pid || [info[(id)kCGWindowOwnerPID] intValue] != pid || ![info[(id)kCGWindowIsOnscreen] boolValue]) continue;
    if (queryStopped() || visited >= maxNodes) { complete = NO; break; }
    visited++;
    NSArray *identity = processIdentity(pid);
    if (!identity) { complete = NO; continue; }
    NSString *found = nil;
    for (NSString *k in c.captureWindows) {
      DWCaptureWindow *r = c.captureWindows[k];
      if (!r.gone && r.pid == pid && r.window.windowID == w.windowID && [r.identity isEqual:identity]) { found = k; r.window = w; break; }
    }
    if (!found) {
      if (c.elements.count + c.captureWindows.count >= 10000) { complete = NO; break; }
      AXUIElementRef app = AXUIElementCreateApplication(pid);
      NSString *ak = key(c, app, nil, nil, nil);
      CFRelease(app);
      if (!ak.length) { complete = NO; continue; }
      DWCaptureWindow *r = [DWCaptureWindow new];
      r.pid = pid; r.identity = identity; r.window = w; r.app = ak;
      found = [@"cw-" stringByAppendingString:NSUUID.UUID.UUIDString];
      c.captureWindows[found] = r;
    }
    [nodes addObject:captureNode(c, found, q[@"Fields"])];
  }
  return @{@"Result": @{@"Nodes": nodes, @"Complete": @(complete), @"Dirty": complete ? @NO : @YES, @"Visited": @(visited), @"Unavailable": complete ? @[] : @[@"capture_window_budget"], @"Seat": seat(c)}};
}
static NSDictionary *capture(DWContext *c, NSDictionary *r, DWCancel *cancel) {
  if (![r[@"Kind"] isEqual:@"visible_region"] && ![r[@"Kind"] isEqual:@"window_content"]) return err(@"capability_unavailable");
  if (!CGPreflightScreenCaptureAccess()) return err(@"permission_denied");
  double deadline = monotonicSeconds() + MAX(1, [r[@"ReadTimeoutMS"] longLongValue]) / 1000.0;
  SCShareableContent *content = shareable(cancel, deadline);
  if (!content) return err(cancelled(cancel) ? @"cancelled" : @"provider_unavailable");
  BOOL window = [r[@"Kind"] isEqual:@"window_content"];
  NSString *k = r[@"Key"];
  DWCaptureWindow *target = c.captureWindows[k];
  // An AX window becomes a capture target only after a same-worker native ID
  // join. Titles, bounds and foreground state never choose the SCWindow.
  NSDictionary *joined = nil;
  if (window && !target) {
    NSDictionary *identity = pocWindowIdentity(c, k);
    joined = identity[@"Result"];
    if (!joined) return identity;
    pid_t pid = [joined[@"PID"] intValue];
    CGWindowID wid = [joined[@"NativeWindowID"] unsignedIntValue];
    NSArray *process = processIdentity(pid);
    if (!process || ![process isEqual:@[joined[@"StartSec"], joined[@"StartUSec"]]])
      return err(@"capture_identity_unavailable");
    SCWindow *matched = nil;
    for (SCWindow *candidate in content.windows) {
      if (candidate.windowID == wid && candidate.owningApplication.processID == pid) {
        if (matched) return err(@"capture_identity_ambiguous");
        matched = candidate;
      }
    }
    if (!matched) return err(@"window_unavailable");
    target = [DWCaptureWindow new];
    target.pid = pid;
    target.identity = process;
    target.window = matched;
    target.app = joined[@"AppKey"];
  }
  if (window) {
    NSString *failure = refreshCaptureRecord(c, target, content);
    if (failure) return err(failure);
  }
  NSMutableArray *images = [NSMutableArray array];
  NSArray *sources = window ? @[target.window] : content.displays;
  for (id source in sources) {
    SCContentFilter *filter;
    CGRect clipped;
    CGFloat pixelScale;
    SCStreamConfiguration *config = [SCStreamConfiguration new];
    if (window) {
      filter = [[SCContentFilter alloc] initWithDesktopIndependentWindow:source];
      clipped = filter.contentRect;
      pixelScale = filter.pointPixelScale;
      config.ignoreShadowsSingleWindow = YES;
      if (@available(macOS 14.2, *)) config.includeChildWindows = NO;
    } else {
      SCDisplay *d = source;
      CGRect frame = CGDisplayBounds(d.displayID);
      clipped = frame;
      NSDictionary *region = r[@"Region"];
      if (region && (id)region != NSNull.null) {
        NSDictionary *b = region[@"Rect"];
        clipped = CGRectIntersection(frame, CGRectMake([b[@"X"] doubleValue], [b[@"Y"] doubleValue], [b[@"Width"] doubleValue], [b[@"Height"] doubleValue]));
      }
      if (CGRectIsEmpty(clipped) || CGRectIsNull(clipped)) continue;
      filter = [[SCContentFilter alloc] initWithDisplay:d excludingWindows:@[]];
      pixelScale = CGDisplayPixelsWide(d.displayID) / frame.size.width;
      config.sourceRect = CGRectMake(clipped.origin.x-frame.origin.x, clipped.origin.y-frame.origin.y, clipped.size.width, clipped.size.height);
    }
    if (CGRectIsEmpty(clipped) || CGRectIsNull(clipped)) return err(@"window_unavailable");
    CGFloat scale = MIN(pixelScale, MIN([r[@"MaxPixelWidth"] doubleValue]/clipped.size.width, [r[@"MaxPixelHeight"] doubleValue]/clipped.size.height));
    config.width = MAX(1, (size_t)(clipped.size.width*scale));
    config.height = MAX(1, (size_t)(clipped.size.height*scale));
    if (config.width * config.height > 16 * 1024 * 1024 || images.count >= 16) return err(@"resource_exhausted");
    config.pixelFormat = kCVPixelFormatType_32BGRA;
    config.colorSpaceName = kCGColorSpaceSRGB;
    config.showsCursor = !window && [r[@"IncludeCursor"] boolValue];
    __block NSData *png = nil;
    __block size_t width = 0, height = 0;
    dispatch_semaphore_t sem = dispatch_semaphore_create(0);
    [SCScreenshotManager captureSampleBufferWithFilter:filter configuration:config completionHandler:^(CMSampleBufferRef sample, NSError *error) {
      if (!error && sample && CMSampleBufferIsValid(sample) && CMSampleBufferDataIsReady(sample)) {
        NSArray *attachments = CFBridgingRelease(CMSampleBufferGetSampleAttachmentsArray(sample, false) ? CFRetain(CMSampleBufferGetSampleAttachmentsArray(sample, false)) : NULL);
        NSNumber *status = attachments.firstObject[SCStreamFrameInfoStatus];
        // Some screenshot providers omit stream metadata. Reject every explicit
        // non-complete status; a one-shot screenshot is never reused or cached.
        if (!status || status.integerValue == SCFrameStatusComplete) {
          CVImageBufferRef buffer = CMSampleBufferGetImageBuffer(sample);
          if (buffer) {
            CIImage *image = [CIImage imageWithCVPixelBuffer:buffer];
            CGColorSpaceRef color = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
            CGImageRef cg = [[CIContext contextWithOptions:nil] createCGImage:image fromRect:image.extent format:kCIFormatRGBA8 colorSpace:color];
            CGColorSpaceRelease(color);
            if (cg) {
              width = CGImageGetWidth(cg); height = CGImageGetHeight(cg);
              NSMutableData *data = [NSMutableData data];
              CGImageDestinationRef dest = CGImageDestinationCreateWithData((__bridge CFMutableDataRef)data, (__bridge CFStringRef)UTTypePNG.identifier, 1, NULL);
              if (dest) { CGImageDestinationAddImage(dest, cg, NULL); if (CGImageDestinationFinalize(dest)) png = data; CFRelease(dest); }
              CGImageRelease(cg);
            }
          }
        }
      }
      dispatch_semaphore_signal(sem);
    }];
    if (!waitNative(sem, cancel, deadline)) return err(cancelled(cancel) ? @"cancelled" : @"capture_timeout");
    if (!png) return err(@"capture_frame_unavailable");
    if (window) {
      CGRect before = target.window.frame;
      SCShareableContent *after = shareable(cancel, deadline);
      if (!after) return err(@"window_unavailable");
      if (joined) {
        NSDictionary *fresh = pocWindowIdentity(c, k)[@"Result"];
        if (!fresh || ![fresh[@"PID"] isEqual:joined[@"PID"]] ||
            ![fresh[@"StartSec"] isEqual:joined[@"StartSec"]] ||
            ![fresh[@"StartUSec"] isEqual:joined[@"StartUSec"]] ||
            ![fresh[@"NativeWindowID"] isEqual:joined[@"NativeWindowID"]] ||
            ![fresh[@"WindowKey"] isEqual:joined[@"WindowKey"]])
          return err(@"capture_identity_changed");
      }
      NSString *failure = refreshCaptureRecord(c, target, after);
      if (failure) return err(failure);
      if (!CGRectEqualToRect(before, target.window.frame)) return err(@"capture_geometry_changed");
    }
    [images addObject:@{@"Bytes": [png base64EncodedStringWithOptions:0], @"ContentType": @"image/png", @"Width": @(width), @"Height": @(height), @"Bounds": @{@"Frame": window ? @"window" : @"desktop", @"Topology": @(c.topology), @"Rect": @{@"X": window ? @0 : @(clipped.origin.x), @"Y": window ? @0 : @(clipped.origin.y), @"Width": @(clipped.size.width), @"Height": @(clipped.size.height)}}}];
  }
  return @{@"Result": images};
}
