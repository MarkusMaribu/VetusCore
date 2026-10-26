/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. https://mozilla.org/MPL/2.0/ */
#import "system_headers.h"
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include <unistd.h>

@interface NSData (AudioTiming)
+ (id)dataWithBytes:(const void *)bytes length:(NSUInteger)length;
@end
@interface AVAudioPlayer : NSObject
- (id)initWithData:(id)data error:(id *)error;
- (void)prepareToPlay;
- (BOOL)play;
- (void)pause;
- (void)stop;
- (BOOL)isPlaying;
- (double)currentTime;
- (void)setCurrentTime:(double)time;
- (void)setDelegate:(id)delegate;
@end
@interface NSRunLoop : NSObject
+ (id)mainRunLoop;
- (void)runUntilDate:(id)date;
@end
@interface NSDate : NSObject
+ (id)dateWithTimeIntervalSinceNow:(double)seconds;
@end

@interface AudioWaitProbe : NSObject @end
@implementation AudioWaitProbe
+ (void)tick:(id)timer {}
@end
static void audio_wait(double seconds) {
  // Independent audio queues do not keep an otherwise empty run loop alive.
  id timer = [NSTimer scheduledTimerWithTimeInterval:seconds + 0.1
    target:[AudioWaitProbe class] selector:@selector(tick:) userInfo:nil repeats:NO];
  [[NSRunLoop mainRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:seconds]];
  [timer invalidate];
}
static AVAudioPlayer *timing_player(unsigned frames) {
  // A complete mono 22050 Hz, 16-bit PCM WAV, without external fixtures.
  unsigned bytes = frames * 2;
  unsigned char *wav = calloc(1, bytes + 44);
  memcpy(wav, "RIFF", 4); *(unsigned *)(wav + 4) = bytes + 36;
  memcpy(wav + 8, "WAVEfmt ", 8); *(unsigned *)(wav + 16) = 16;
  *(unsigned short *)(wav + 20) = 1; *(unsigned short *)(wav + 22) = 1;
  *(unsigned *)(wav + 24) = 22050; *(unsigned *)(wav + 28) = 44100;
  *(unsigned short *)(wav + 32) = 2; *(unsigned short *)(wav + 34) = 16;
  memcpy(wav + 36, "data", 4); *(unsigned *)(wav + 40) = bytes;
  id data = [NSData dataWithBytes:wav length:bytes + 44];
  free(wav);
  return [[AVAudioPlayer alloc] initWithData:data error:NULL];
}

// Playback position must follow consumed samples, not buffers read ahead.
// https://developer.apple.com/documentation/avfaudio/avaudioplayer/currenttime
int test_AVAudioPlayer_playback_clock_and_seek(void) {
  AVAudioPlayer *player = timing_player(22050 * 3);
  if (!player) return -1;
  int result = 0;
  [player prepareToPlay];
  if (fabs([player currentTime]) > 0.001) { result = -2; goto cleanup; }
  if (![player play]) { result = -3; goto cleanup; }
  if ([player currentTime] > 0.15) { result = -4; goto cleanup; }
  audio_wait(0.12);
  double time = [player currentTime];
  if (time < 0.04 || time > 0.35) { result = -5; goto cleanup; }
  [player pause];
  double paused = [player currentTime];
  audio_wait(0.08);
  if (fabs([player currentTime] - paused) > 0.01) { result = -6; goto cleanup; }
  [player setCurrentTime:1.0];
  if (fabs([player currentTime] - 1.0) > 0.01) { result = -7; goto cleanup; }
  if (![player play]) { result = -8; goto cleanup; }
  audio_wait(0.12);
  time = [player currentTime];
  if (time < 1.04 || time > 1.35) { result = -9; goto cleanup; }
  [player setCurrentTime:0.4];
  audio_wait(0.12);
  time = [player currentTime];
  if (time < 0.44 || time > 0.75) { result = -10; goto cleanup; }
  // Seek to a fractional sample just before EOF. Let hardware drain without
  // pumping the finish callback: packet rounding must not wrap time to zero.
  [player setCurrentTime:2.99999];
  usleep(100000);
  time = [player currentTime];
  if (time < 2.99 || time > 3.0) { result = -11; goto cleanup; }
cleanup:
  [player stop]; [player release];
  return result;
}

static int finished_calls;
@interface AudioFinishProbe : NSObject @end
@implementation AudioFinishProbe
- (void)audioPlayerDidFinishPlaying:(id)player successfully:(BOOL)ok {
  finished_calls += ok ? 1 : 100;
}
@end

// Finishing means all samples played, including audio already buffered.
// https://developer.apple.com/documentation/avfaudio/avaudioplayerdelegate/audioplayerdidfinishplaying(_:successfully:)
int test_AVAudioPlayer_finish_waits_for_drain(void) {
  AVAudioPlayer *player = timing_player(22050 / 2);
  id probe = [AudioFinishProbe new];
  finished_calls = 0;
  [player setDelegate:probe];
  int result = 0;
  [player prepareToPlay];
  if (finished_calls || [player isPlaying]) { result = -1; goto cleanup; }
  if (![player play]) { result = -2; goto cleanup; }
  audio_wait(0.12);
  if (finished_calls || ![player isPlaying]) { result = -3; goto cleanup; }
  audio_wait(0.65);
  if (finished_calls != 1 || [player isPlaying]) { result = -4; goto cleanup; }
  // The same player can start again after naturally finishing.
  if (![player play]) { result = -5; goto cleanup; }
  audio_wait(0.12);
  if (finished_calls != 1 || ![player isPlaying]) { result = -6; goto cleanup; }
  [player stop];
  audio_wait(0.1);
  if (finished_calls != 1) result = -7;
cleanup:
  [player stop]; [player release]; [probe release];
  return result;
}
