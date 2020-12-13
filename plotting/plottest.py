import matplotlib.pyplot as plt
import math

A4_PITCH = 69
A4_FREQ = 440.0
FS = 44100


def midi_pitch_to_freq(pitch):

    # Midi notes can be 0-127
    return (2 ** ((pitch - A4_PITCH) / 12.0)) * A4_FREQ


def f(t, note_value):
    return math.sin(t * midi_pitch_to_freq(note_value) * math.tau)


def saw(n):
    return (((n + math.pi) % math.tau) / math.pi) - 1.0


def square(n):
    return min(2.0, max(0.0, math.sin(n) * 100.0)) - 1.0


def triangle(n):
    return abs(saw(n + math.pi / 2.0)) * 2.0 - 1.0


def f2(t, note_value):
    return saw(t * midi_pitch_to_freq(note_value) * math.tau)


def f3(t, note_value):
    return square(t * midi_pitch_to_freq(note_value) * math.tau)


def f4(t, note_value):
    return triangle(t * midi_pitch_to_freq(note_value) * math.tau)


plt.plot([f(t / FS, 55) for t in range(1000)])
plt.plot([f2(t / FS, 55) for t in range(1000)])
# plt.plot([f4(t / FS, 55) for t in range(1000)])
plt.show()
