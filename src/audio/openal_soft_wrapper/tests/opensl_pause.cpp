/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
// Android regression test for the bundled OpenSL backend's stop/start lifecycle.
// Link against the same static OpenAL build as VetusCore, then run on Android.
// https://openal-soft.org/openal-extensions/SOFT_pause_device.txt
#include <AL/al.h>
#include <AL/alc.h>
#include <AL/alext.h>
#include <cassert>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <thread>
#include <vector>

int main()
{
    setenv("ALSOFT_DRIVERS", "opensl", 1);
    auto *device = alcOpenDevice(nullptr);
    assert(device);
    auto *context = alcCreateContext(device, nullptr);
    assert(context && alcMakeContextCurrent(context));
    auto pause = reinterpret_cast<LPALCDEVICEPAUSESOFT>(
        alcGetProcAddress(device, "alcDevicePauseSOFT"));
    auto resume = reinterpret_cast<LPALCDEVICERESUMESOFT>(
        alcGetProcAddress(device, "alcDeviceResumeSOFT"));
    assert(pause && resume);
    std::vector<short> samples(44100, 1000);
    ALuint buffer, source;
    alGenBuffers(1, &buffer);
    alBufferData(buffer, AL_FORMAT_MONO16, samples.data(), samples.size() * 2, 44100);
    alGenSources(1, &source);
    alSourcei(source, AL_BUFFER, buffer);
    alSourcei(source, AL_LOOPING, AL_TRUE);
    alSourcePlay(source);
    auto offset = [&] {
        ALint value;
        alGetSourcei(source, AL_SAMPLE_OFFSET, &value);
        assert(alGetError() == AL_NO_ERROR);
        return value;
    };
    for(int cycle = 0; cycle < 5; ++cycle)
    {
        std::this_thread::sleep_for(std::chrono::milliseconds(150));
        pause(device);
        pause(device); // The extension requires idempotence.
        auto parked = offset();
        std::this_thread::sleep_for(std::chrono::milliseconds(150));
        assert(offset() == parked);
        resume(device);
        resume(device);
        std::this_thread::sleep_for(std::chrono::milliseconds(150));
        auto playing = offset();
        assert(playing != parked);
        std::printf("cycle %d: paused=%d, resumed=%d\n", cycle + 1, parked, playing);
    }
    alDeleteSources(1, &source);
    alDeleteBuffers(1, &buffer);
    alcMakeContextCurrent(nullptr);
    alcDestroyContext(context);
    assert(alcCloseDevice(device));
    std::puts("OpenSL pause/resume passed");
}
