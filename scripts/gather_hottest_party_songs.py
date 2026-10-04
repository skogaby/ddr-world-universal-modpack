#!/usr/bin/env python3
"""Gather Dancing Stage / DDR HOTTEST PARTY's per-song data into one folder, named
"Artist - Title": the step chart (`.ssq`), the full song audio (`.wav`) and the song-select
banner (`.png`), plus an index.csv (song, artist, title, BPM, files).

Reads an extraction made by `scripts/extract_wii_ddr_data.py extract <disc> <out> --png --wav`
(formats and RE: docs/wii_ddr_hottest_party_research.md):
  - chart:  data/c_000_NN/000.ssq (entry 0 of the song's bundle data/c_000_NN.bin);
  - audio:  sound/ddr_brsar/<group>/fileNNN.000.wav of sound MU_DDR_<a+1>, where `a` is the
            song table's audio index (dol/songs.csv; sound -> file in sound/ddr_brsar/sounds.csv);
  - banner: data/selcom/<008 + i>.00.png, the 226 x 110 banners in song order (the three
            tokyoEVOLVED versions share one).
Songs 53..55 (True Love, Pluto The First, DOUBLE TORNARD) are Japan-only: the EU disc keeps their
charts and banners but not their audio. The disc has no artist strings, so the artists are the
ones drawn on the banners (SONGS below); titles are the song table's, with its typos fixed and
characters Windows cannot hold in a file name replaced.

Usage:
    gather_hottest_party_songs.py [extracted-dir] [out-dir]
      defaults: ~/Desktop/DDR Wii ISOs/hottest_party_extracted, ~/Desktop/DDR Wii ISOs/hottest_party_songs
"""
import argparse
import csv
import os
import shutil

# song -> (artist, title). Artists are the in-game artist names (drawn on the banners only; the
# disc has no artist strings), titles the game's own (main.dol song table) with its typos fixed
# and characters Windows cannot hold in a file name replaced.
SONGS = {
    1: ('Lady-S', '1, 2 Step'), 2: ('M-Crew project', '99 Red Balloons'), 3: ('T.R.Master MC', 'Clocks'),
    4: ("Club 90's", 'Finally'), 5: ('Rave Attackers', 'Gonna Make You Sweat (Everybody Dance Now)'),
    6: ('Disco Queen', 'Hot Stuff'), 7: ('Happy Happy Cores', 'Karma Chameleon'), 8: ('Trance Jack', 'Nothing But You'),
    9: ('Spots', 'Rhythm Is a Dancer'), 10: ("Eazin'", 'Summertime'), 11: ('Honey Sweets', 'The Sign'),
    12: ('M.A.N', "You Spin Me 'Round (Like a Record)"), 13: ('Traveler', 'Far Away'),
    14: ('Jet Rockers', 'Lips of an Angel'), 15: ("2000's Stars", 'Call On Me'), 16: ('Cools K', 'Buried a Lie'),
    17: ('OK OK OK', 'Disco Inferno'), 18: ("Man's Cool", 'Always (Microbots Trance Dance Mix)'),
    19: ('Single Funk', 'Little L'), 20: ('Smooth-1', 'Yo, Excuse Me Miss'), 21: ('Neo-Gruv', 'Gypsy Woman'),
    22: ('Stopped Cold', 'Caught Up'), 23: ('Life Aloud', "I Don't Feel Like Dancin'"), 24: ('WG', 'Blue Monday'),
    25: ('Okokoro', 'Too Little, Too Late'), 26: ('Wrapped Up', 'Unappreciated'),
    27: ('U.T.D & Friends', 'Lesson by DJ'), 28: ('W.W.S', 'LOVE SHINE (Body Grooverz 2006 mix)'),
    29: ('Ele Rocks', 'BREAK DOWN! (World Version)'), 30: ('J-Ravers', '1998 (Sparky 2006)'),
    31: ('The Sweetest', 'CANDY (UFO mix)'), 32: ('J-Ravers', 'B4U (The Acolyte mix)'),
    33: ('Trance Star', 'Confession'), 34: ('True Dreamer', 'let it out'), 35: ('NAOKI', 'will'),
    36: ('Freeman', 'little steps'), 37: ('The Lonely Hearts', "Touchin'"), 38: ('Stepper', "I'm Flying Away"),
    39: ('Happy CoreMan', 'We Will Live Together'), 40: ('The Lonely Hearts', 'Heavens and the Earth'),
    41: ('J.J. Pops', 'Moving On'), 42: ('NM feat. Malaya', 'Here I Go Again'), 43: ('Sparky', 'the beat'),
    44: ('NM feat. Alison Wade', 'Beautiful Inside (Cube Hard Mix)'), 45: ('Latenighter', 'Mess With My Emotions'),
    46: ('Black Rose Garden', 'THE REASON'), 47: ('Jun', 'SUPER SAMURAI'), 48: ('U1', 'Such A Feeling'),
    49: ('800 Slopes', 'Hold Tight'), 50: ('NAOKI underground', 'tokyoEVOLVED (Ver.A)'),
    51: ('NAOKI underground', 'tokyoEVOLVED (Ver.B)'), 52: ('NAOKI underground', 'tokyoEVOLVED (Ver.C)'),
    53: ('Jun feat. Schanita', "True Love (Clubstar's True Club Mix)"), 54: ('White Wall', 'Pluto The First'),
    55: ('Evo-X', 'DOUBLE TORNARD'),
}
JP_ONLY = {53, 54, 55}   # charts / banners left on the EU disc; their audio slot is tokyoEVOLVED's
BANNER = {s: s - 1 for s in range(1, 51)}   # selcom entry 8 + banner index; the tokyoEVOLVEDs share one
BANNER.update({51: 49, 52: 49, 53: 50, 54: 51, 55: 52})
BAD = '<>:"/\\|?*'


def safe(s):
    return ''.join('-' if c in BAD else c for c in s).strip(' .')


def _tilde(path):
    home = os.path.expanduser('~')
    path = os.path.abspath(path)
    return '~' + path[len(home):] if path.startswith(home) else path


def gather(X, OUT):
    songs = {int(r['song']): r for r in csv.DictReader(open(os.path.join(X, 'dol', 'songs.csv')))}
    sounds = {r['name']: r for r in csv.DictReader(open(os.path.join(X, 'sound', 'ddr_brsar', 'sounds.csv')))}
    brsar = os.path.join(X, 'sound', 'ddr_brsar')
    file_dir = {}
    for g in os.listdir(brsar):
        gd = os.path.join(brsar, g)
        if os.path.isdir(gd):
            for f in os.listdir(gd):
                if f.endswith('.rwsd'):
                    file_dir[int(f[4:7])] = gd
    os.makedirs(OUT, exist_ok=True)
    rows = []
    for n in sorted(songs):
        r = songs[n]
        artist, title = SONGS[n]
        stem = safe('%s - %s' % (artist, title))
        got = []
        shutil.copyfile(os.path.join(X, 'data', 'c_000_%02d' % n, '000.ssq'), os.path.join(OUT, stem + '.ssq'))
        got.append('ssq')
        if n not in JP_ONLY:
            snd = sounds['MU_DDR_%03d' % (int(r['a']) + 1)]
            wav = os.path.join(file_dir[int(snd['file'])], 'file%03d.000.wav' % int(snd['file']))
            shutil.copyfile(wav, os.path.join(OUT, stem + '.wav'))
            got.append('wav')
        banner = os.path.join(X, 'data', 'selcom', '%03d.00.png' % (8 + BANNER[n]))
        shutil.copyfile(banner, os.path.join(OUT, stem + '.png'))
        got.append('banner')
        bpm = r['bpm_hi'] if r['bpm_lo'] in ('0', r['bpm_hi']) else '%s-%s' % (r['bpm_lo'], r['bpm_hi'])
        rows.append([n, artist, title, bpm, 'JP-only (no audio on the EU disc)' if n in JP_ONLY else '', ' '.join(got)])
        print('%2d %-60s %s' % (n, stem, ' '.join(got)))
    with open(os.path.join(OUT, 'index.csv'), 'w', newline='') as f:
        w = csv.writer(f)
        w.writerow(['song', 'artist', 'title', 'bpm', 'note', 'files'])
        w.writerows(rows)
    print('%d songs -> %s' % (len(rows), _tilde(OUT)))


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0],
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('extracted', nargs='?', default='~/Desktop/DDR Wii ISOs/hottest_party_extracted')
    ap.add_argument('out', nargs='?', default='~/Desktop/DDR Wii ISOs/hottest_party_songs')
    a = ap.parse_args()
    gather(os.path.expanduser(a.extracted), os.path.expanduser(a.out))


if __name__ == '__main__':
    main()
