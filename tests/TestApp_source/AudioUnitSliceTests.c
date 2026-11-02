/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

#include <pthread.h>
#include <string.h>
#include <unistd.h>

typedef unsigned int UInt32;
typedef int OSStatus;
typedef void *AudioUnit;
typedef struct { UInt32 type, subtype, manufacturer, flags, mask; } ComponentDescription;
typedef struct {
  double sampleRate;
  UInt32 format, flags, packetBytes, packetFrames, frameBytes, channels, bits, reserved;
} StreamFormat;
typedef struct { UInt32 channels, size; void *data; } AudioBuffer;
typedef struct { UInt32 count; AudioBuffer buffers[1]; } AudioBufferList;
typedef OSStatus (*RenderCallback)(void *, UInt32 *, const void *, UInt32,
                                   UInt32, AudioBufferList *);
typedef struct { RenderCallback callback; void *context; } RenderCallbackStruct;
void *AudioComponentFindNext(void *, const ComponentDescription *);
OSStatus AudioComponentInstanceNew(void *, AudioUnit *);
OSStatus AudioComponentInstanceDispose(AudioUnit);
OSStatus AudioUnitSetProperty(AudioUnit, UInt32, UInt32, UInt32, const void *, UInt32);
OSStatus AudioUnitInitialize(AudioUnit);
OSStatus AudioUnitUninitialize(AudioUnit);
OSStatus AudioOutputUnitStart(AudioUnit);
OSStatus AudioOutputUnitStop(AudioUnit);

typedef struct {
  pthread_mutex_t mutex;
  UInt32 maximum, callbacks, oversized;
} SliceCheck;

static OSStatus check_slice(void *context, UInt32 *flags, const void *timestamp,
                            UInt32 bus, UInt32 frames, AudioBufferList *data) {
  SliceCheck *check = context;
  pthread_mutex_lock(&check->mutex);
  check->callbacks++;
  check->oversized |= frames > check->maximum;
  pthread_mutex_unlock(&check->mutex);
  for (UInt32 i = 0; i < data->count; i++)
    if (data->buffers[i].data)
      memset(data->buffers[i].data, 0, data->buffers[i].size);
  return 0;
}

// QA1533 says audio units size internal buffers from MaximumFramesPerSlice
// and must not pull larger input slices. Check both the unit callback and
// input-bus callback paths, using limits below the nominal hardware slice.
// https://developer.apple.com/library/archive/qa/qa1533/_index.html
// Upstream comparison (stored/exposed property, original render path):
// https://github.com/touchHLE/touchHLE/blob/trunk/src/frameworks/audio_toolbox/audio_unit.rs
int test_audio_unit_maximum_frames_per_slice(void) {
  for (UInt32 input_bus = 0; input_bus < 2; input_bus++) {
    ComponentDescription description = {'auou', 'rioc', 'appl', 0, 0};
    void *component = AudioComponentFindNext(NULL, &description);
    AudioUnit unit;
    if (!component || AudioComponentInstanceNew(component, &unit)) return -1;
    SliceCheck check = {PTHREAD_MUTEX_INITIALIZER, input_bus ? 128 : 32, 0, 0};
    RenderCallbackStruct callback = {check_slice, &check};
    StreamFormat format = {44100, 'lpcm', 12, 2, 1, 2, 1, 16, 0};
    int result = 0, initialized = 0, started = 0;
    if (AudioUnitSetProperty(unit, 14, 0, 0, &check.maximum, sizeof(check.maximum)) ||
        AudioUnitSetProperty(unit, 8, 1, 0, &format, sizeof(format)) ||
        AudioUnitSetProperty(unit, 23, input_bus, 0, &callback, sizeof(callback))) {
      result = -2; goto cleanup;
    }
    if (AudioUnitInitialize(unit)) { result = -3; goto cleanup; }
    initialized = 1;
    if (AudioOutputUnitStart(unit)) { result = -4; goto cleanup; }
    started = 1;
    usleep(200000);
    if (AudioOutputUnitStop(unit)) { result = -5; goto cleanup; }
    started = 0;
    pthread_mutex_lock(&check.mutex);
    if (check.callbacks == 0 || check.oversized) result = input_bus ? -7 : -6;
    pthread_mutex_unlock(&check.mutex);
cleanup:
    if (started) AudioOutputUnitStop(unit);
    if (initialized) AudioUnitUninitialize(unit);
    AudioComponentInstanceDispose(unit);
    pthread_mutex_destroy(&check.mutex);
    if (result) return result;
  }
  return 0;
}
