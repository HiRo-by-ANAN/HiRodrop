#import <AppKit/AppKit.h>
#import <CoreServices/CoreServices.h>
#import <stdint.h>
#import <stdlib.h>

static NSString *const HiRodropHandoffPasteboardName = @"com.hirodrop.desktop.share-handoff";

typedef void (*HiRodropFilesCallback)(const char *const *paths, uintptr_t count);

static void HiRodropActivate(void) {
    [NSApp activateIgnoringOtherApps:YES];
    for (NSWindow *window in NSApp.windows) {
        if (window.canBecomeKeyWindow) {
            [window makeKeyAndOrderFront:nil];
            break;
        }
    }
}

static BOOL HiRodropDeliverPasteboard(NSPasteboard *pasteboard,
                                      HiRodropFilesCallback callback) {
    NSDictionary *options = @{ NSPasteboardURLReadingFileURLsOnlyKey : @YES };
    NSArray<NSURL *> *urls = [pasteboard readObjectsForClasses:@[ NSURL.class ]
                                                       options:options];
    NSLog(@"HiRodrop received pasteboard types=%@ URL count=%lu",
          pasteboard.types,
          (unsigned long)urls.count);
    NSMutableArray<NSURL *> *fileURLs = [NSMutableArray arrayWithCapacity:urls.count];
    for (NSURL *url in urls) {
        NSURL *pathURL = url.isFileURL ? [NSURL fileURLWithPath:url.path] : nil;
        pathURL = pathURL.URLByStandardizingPath;
        if (pathURL.fileSystemRepresentation != NULL) {
            [fileURLs addObject:pathURL];
        }
    }

    if (fileURLs.count == 0) {
        NSString *rawURL = [pasteboard stringForType:NSPasteboardTypeFileURL];
        NSURL *url = rawURL == nil ? nil : [NSURL URLWithString:rawURL];
        NSURL *pathURL = url.isFileURL ? [NSURL fileURLWithPath:url.path] : nil;
        if (pathURL.fileSystemRepresentation != NULL) {
            [fileURLs addObject:pathURL.URLByStandardizingPath];
        }
    }

    if (fileURLs.count == 0 || callback == NULL) {
        return NO;
    }
    const char **paths = calloc(fileURLs.count, sizeof(*paths));
    if (paths == NULL) {
        return NO;
    }
    for (NSUInteger index = 0; index < fileURLs.count; index++) {
        paths[index] = fileURLs[index].fileSystemRepresentation;
    }
    callback(paths, (uintptr_t)fileURLs.count);
    free(paths);
    return YES;
}

@interface HiRodropServiceProvider : NSObject
@property(nonatomic, assign) HiRodropFilesCallback callback;
@end

@implementation HiRodropServiceProvider

- (void)sendFiles:(NSPasteboard *)pasteboard
         userData:(NSString *)userData
            error:(NSString **)error {
    (void)userData;
    if (!HiRodropDeliverPasteboard(pasteboard, self.callback)) {
        if (error != NULL) {
            *error = @"HiRodrop could not read file URLs from Finder.";
        }
        return;
    }
    HiRodropActivate();
}

- (void)handleGetURLEvent:(NSAppleEventDescriptor *)event
           withReplyEvent:(NSAppleEventDescriptor *)replyEvent {
    (void)replyEvent;
    NSString *rawURL = [event paramDescriptorForKeyword:keyDirectObject].stringValue;
    NSURL *url = rawURL == nil ? nil : [NSURL URLWithString:rawURL];
    if (![url.scheme.lowercaseString isEqualToString:@"hirodrop"] ||
        ![url.host.lowercaseString isEqualToString:@"share-extension"]) {
        return;
    }
    NSPasteboard *pasteboard = [NSPasteboard pasteboardWithName:HiRodropHandoffPasteboardName];
    if (HiRodropDeliverPasteboard(pasteboard, self.callback)) {
        [pasteboard clearContents];
    }
    HiRodropActivate();
}

@end

static HiRodropServiceProvider *serviceProvider;

void hirodrop_install_service_provider(HiRodropFilesCallback callback) {
    serviceProvider = [[HiRodropServiceProvider alloc] init];
    serviceProvider.callback = callback;
    [NSApp setServicesProvider:serviceProvider];
    [NSApp registerServicesMenuSendTypes:@[ NSPasteboardTypeFileURL ] returnTypes:@[]];
    [[NSAppleEventManager sharedAppleEventManager]
        setEventHandler:serviceProvider
             andSelector:@selector(handleGetURLEvent:withReplyEvent:)
           forEventClass:kInternetEventClass
              andEventID:kAEGetURL];

    NSPasteboard *handoff = [NSPasteboard pasteboardWithName:HiRodropHandoffPasteboardName];
    if (HiRodropDeliverPasteboard(handoff, callback)) {
        [handoff clearContents];
        HiRodropActivate();
    }
}
