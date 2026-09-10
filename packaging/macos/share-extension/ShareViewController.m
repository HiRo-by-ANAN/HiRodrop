#import <AppKit/AppKit.h>

static NSString *const HiRodropServiceName = @"Send with HiRodrop";
static NSString *const HiRodropFileURLType = @"public.file-url";
static NSString *const HiRodropHandoffPasteboardName = @"com.hirodrop.desktop.share-handoff";

@interface ShareViewController : NSViewController
@property(nonatomic, strong) NSTextField *statusLabel;
@property(nonatomic, strong) NSProgressIndicator *progressIndicator;
@property(nonatomic, strong) NSButton *retryButton;
@property(nonatomic, strong) NSButton *cancelButton;
@property(nonatomic, strong) NSMutableArray<NSURL *> *fileURLs;
@property(nonatomic, assign) NSUInteger pendingProviders;
@property(nonatomic, assign) BOOL started;
@end

@implementation ShareViewController

- (void)loadView {
    NSView *root = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, 420, 168)];

    NSTextField *title = [NSTextField labelWithString:NSLocalizedString(@"ShareTitle", nil)];
    title.font = [NSFont systemFontOfSize:17 weight:NSFontWeightSemibold];
    title.translatesAutoresizingMaskIntoConstraints = NO;
    [root addSubview:title];

    self.statusLabel = [NSTextField wrappingLabelWithString:NSLocalizedString(@"PreparingFiles", nil)];
    self.statusLabel.textColor = NSColor.secondaryLabelColor;
    self.statusLabel.alignment = NSTextAlignmentCenter;
    self.statusLabel.translatesAutoresizingMaskIntoConstraints = NO;
    [root addSubview:self.statusLabel];

    self.progressIndicator = [[NSProgressIndicator alloc] initWithFrame:NSZeroRect];
    self.progressIndicator.style = NSProgressIndicatorStyleSpinning;
    self.progressIndicator.controlSize = NSControlSizeSmall;
    self.progressIndicator.indeterminate = YES;
    self.progressIndicator.translatesAutoresizingMaskIntoConstraints = NO;
    [self.progressIndicator startAnimation:nil];
    [root addSubview:self.progressIndicator];

    self.retryButton = [NSButton buttonWithTitle:NSLocalizedString(@"Retry", nil)
                                          target:self
                                          action:@selector(retry:)];
    self.retryButton.bezelStyle = NSBezelStyleRounded;
    self.retryButton.hidden = YES;
    self.retryButton.translatesAutoresizingMaskIntoConstraints = NO;
    [root addSubview:self.retryButton];

    self.cancelButton = [NSButton buttonWithTitle:NSLocalizedString(@"Cancel", nil)
                                           target:self
                                           action:@selector(cancel:)];
    self.cancelButton.bezelStyle = NSBezelStyleRounded;
    self.cancelButton.translatesAutoresizingMaskIntoConstraints = NO;
    [root addSubview:self.cancelButton];

    [NSLayoutConstraint activateConstraints:@[
        [title.topAnchor constraintEqualToAnchor:root.topAnchor constant:22],
        [title.centerXAnchor constraintEqualToAnchor:root.centerXAnchor],
        [self.progressIndicator.topAnchor constraintEqualToAnchor:title.bottomAnchor constant:18],
        [self.progressIndicator.centerXAnchor constraintEqualToAnchor:root.centerXAnchor],
        [self.statusLabel.topAnchor constraintEqualToAnchor:self.progressIndicator.bottomAnchor constant:12],
        [self.statusLabel.leadingAnchor constraintEqualToAnchor:root.leadingAnchor constant:28],
        [self.statusLabel.trailingAnchor constraintEqualToAnchor:root.trailingAnchor constant:-28],
        [self.retryButton.bottomAnchor constraintEqualToAnchor:root.bottomAnchor constant:-18],
        [self.retryButton.trailingAnchor constraintEqualToAnchor:root.trailingAnchor constant:-22],
        [self.cancelButton.bottomAnchor constraintEqualToAnchor:root.bottomAnchor constant:-18],
        [self.cancelButton.trailingAnchor constraintEqualToAnchor:self.retryButton.leadingAnchor constant:-10],
    ]];

    self.view = root;
}

- (void)viewDidAppear {
    [super viewDidAppear];
    if (!self.started) {
        self.started = YES;
        [self loadSharedFiles];
    }
}

- (void)loadSharedFiles {
    self.fileURLs = [NSMutableArray array];
    self.pendingProviders = 0;
    self.retryButton.hidden = YES;
    self.progressIndicator.hidden = NO;
    [self.progressIndicator startAnimation:nil];
    self.statusLabel.stringValue = NSLocalizedString(@"PreparingFiles", nil);

    NSMutableArray<NSItemProvider *> *providers = [NSMutableArray array];
    for (NSExtensionItem *item in self.extensionContext.inputItems) {
        for (NSItemProvider *provider in item.attachments) {
            if (provider.registeredTypeIdentifiers.count > 0) {
                [providers addObject:provider];
            }
        }
    }

    if (providers.count == 0) {
        [self showFailure:NSLocalizedString(@"NoFiles", nil)];
        return;
    }

    self.pendingProviders = providers.count;
    __weak typeof(self) weakSelf = self;
    for (NSItemProvider *provider in providers) {
        [self loadFileURLFromProvider:provider completion:^(NSURL *url, NSError *error) {
            dispatch_async(dispatch_get_main_queue(), ^{
                ShareViewController *strongSelf = weakSelf;
                if (strongSelf == nil) {
                    return;
                }
                if (error == nil && url.isFileURL) {
                    NSURL *pathURL = [NSURL fileURLWithPath:url.path];
                    [strongSelf.fileURLs addObject:pathURL.URLByStandardizingPath];
                }
                strongSelf.pendingProviders -= 1;
                if (strongSelf.pendingProviders == 0) {
                    [strongSelf handOffToHiRodrop];
                }
            });
        }];
    }
}

- (void)loadFileURLFromProvider:(NSItemProvider *)provider
                     completion:(void (^)(NSURL *url, NSError *error))completion {
    if ([provider hasItemConformingToTypeIdentifier:HiRodropFileURLType]) {
        [provider loadItemForTypeIdentifier:HiRodropFileURLType
                                    options:nil
                          completionHandler:^(id<NSSecureCoding> item, NSError *error) {
            NSURL *url = [self fileURLFromItem:(id)item];
            completion(url, error);
        }];
        return;
    }

    [self loadInPlaceURLFromProvider:provider
                               types:provider.registeredTypeIdentifiers
                               index:0
                          completion:completion];
}

- (NSURL *)fileURLFromItem:(id)item {
    NSURL *url = nil;
    if ([item isKindOfClass:NSURL.class]) {
        url = item;
    } else if ([item isKindOfClass:NSString.class]) {
        url = [NSURL URLWithString:item];
    } else if ([item isKindOfClass:NSData.class]) {
        NSData *data = item;
        url = [NSURL URLWithDataRepresentation:data relativeToURL:nil];
        if (!url.isFileURL) {
            BOOL stale = NO;
            url = [NSURL URLByResolvingBookmarkData:data
                                            options:NSURLBookmarkResolutionWithoutUI
                                      relativeToURL:nil
                                bookmarkDataIsStale:&stale
                                              error:nil];
        }
        if (!url.isFileURL) {
            NSString *text = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
            url = text == nil ? nil : [NSURL URLWithString:text];
        }
    }
    return url.isFileURL ? url : nil;
}

- (void)loadInPlaceURLFromProvider:(NSItemProvider *)provider
                              types:(NSArray<NSString *> *)types
                              index:(NSUInteger)index
                         completion:(void (^)(NSURL *url, NSError *error))completion {
    if (index >= types.count) {
        NSError *error = [NSError errorWithDomain:NSCocoaErrorDomain
                                             code:NSFileReadUnknownError
                                         userInfo:nil];
        completion(nil, error);
        return;
    }

    [provider loadInPlaceFileRepresentationForTypeIdentifier:types[index]
                                            completionHandler:^(NSURL *url,
                                                                BOOL isInPlace,
                                                                NSError *error) {
        (void)isInPlace;
        if (error == nil && url.isFileURL) {
            completion(url, nil);
        } else {
            [self loadInPlaceURLFromProvider:provider
                                       types:types
                                       index:index + 1
                                  completion:completion];
        }
    }];
}

- (void)handOffToHiRodrop {
    if (self.fileURLs.count == 0) {
        [self showFailure:NSLocalizedString(@"UnreadableFiles", nil)];
        return;
    }

    self.statusLabel.stringValue = NSLocalizedString(@"OpeningHiRodrop", nil);
    NSPasteboard *pasteboard = [NSPasteboard pasteboardWithName:HiRodropHandoffPasteboardName];
    [pasteboard clearContents];
    if (![pasteboard writeObjects:self.fileURLs]) {
        [self showFailure:NSLocalizedString(@"ServiceUnavailable", nil)];
        return;
    }
    __weak typeof(self) weakSelf = self;
    [self.extensionContext openURL:[NSURL URLWithString:@"hirodrop://share-extension"]
                 completionHandler:^(BOOL success) {
        dispatch_async(dispatch_get_main_queue(), ^{
            ShareViewController *strongSelf = weakSelf;
            if (success) {
                [strongSelf.extensionContext completeRequestReturningItems:@[]
                                                         completionHandler:nil];
            } else if (!NSPerformService(HiRodropServiceName, pasteboard)) {
                [strongSelf showFailure:NSLocalizedString(@"ServiceUnavailable", nil)];
            } else {
                [strongSelf.extensionContext completeRequestReturningItems:@[]
                                                         completionHandler:nil];
            }
        });
    }];
}

- (void)showFailure:(NSString *)message {
    [self.progressIndicator stopAnimation:nil];
    self.progressIndicator.hidden = YES;
    self.statusLabel.stringValue = message;
    self.retryButton.hidden = NO;
}

- (void)retry:(id)sender {
    (void)sender;
    [self loadSharedFiles];
}

- (void)cancel:(id)sender {
    (void)sender;
    NSError *error = [NSError errorWithDomain:NSCocoaErrorDomain
                                         code:NSUserCancelledError
                                     userInfo:nil];
    [self.extensionContext cancelRequestWithError:error];
}

@end
