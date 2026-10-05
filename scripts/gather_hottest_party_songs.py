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

FuruFuru Party (HOTTEST PARTY 2, RD4JA4) and MUSIC FIT (HOTTEST PARTY 3, RJRJA4) run on Konami's
zan engine (docs/wii_ddr_hottest_party_2_3_research.md) and are detected from the extraction's
layout. Extract them with
    extract_wii_ddr_data.py extract <disc dir> <work> --only sound,ssq,select --png      (HP2)
    extract_wii_ddr_data.py extract <disc dir> <work> --only sound,ssq,2Dcommon --png    (HP3)
(--wav is not needed: the song waves are decoded here). Song n of the main.dol song table
(FuruFuru Party 0x801e3200, 32-byte rows; MUSIC FIT 0x80213f98, 140-byte rows) is:
  - chart:  ssq/MU_DDR_nnn.ss9 (HP2); HP3: ssq/Jss9/MU_DDR_nnnJ.ss9 where it exists (songs 1-28,
            whose Japanese audio cut differs), else ssq/ss9/MU_DDR_nnn.ss9. `.ss9` is plain SSQ.
            HP3's B / R / JB / JR / L variants (other play modes) are not gathered;
  - audio:  wave 0 of sound MU_DDR_nnn (HP2) / MU_DDR_nnnJ (HP3) in the brsar (RWSD 1.3 + RWAR);
  - banner: HP2 the 226 x 110 banner in select/select_cmn_jp.bin (BANNER2); HP3 the 256 x 256
            jacket comAF_JP.bin #008.<n-1> in 2Dcommon/.
The titles and artists are the games' own, read off the song title strips (HP2 ssq/SSQnnn.bin,
HP3 comAF_JP.bin #009); MUSIC FIT's table slots 5, 29, 30, 50-52, 59, 60 are cut songs (a shared
dummy wave, no jacket or title) and are skipped.

With --csv (a DDR World song list: `id,basename,title,,artist,...`, `//,,<game>` block headers),
songs are named `<basename>.ssq/.wav/.png` in out-dir when the list has them (matched by title,
the game's block first) and go to `<out-dir>_unassigned` as "Title" otherwise.

Usage:
    gather_hottest_party_songs.py [extracted-dir] [out-dir] [--csv list.csv] [--unassigned dir]
      defaults: ~/Desktop/DDR Wii ISOs/hottest_party_extracted, ~/Desktop/DDR Wii ISOs/hottest_party_songs
"""
import argparse
import csv
import os
import re
import shutil
import sys

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


# ---------------------------------------------------------------------------
# FuruFuru Party (HOTTEST PARTY 2) / MUSIC FIT (HOTTEST PARTY 3)
# ---------------------------------------------------------------------------
# song -> (title, artist, list title). Title / artist as on the game's title strip; the list title
# is the English name a DDR World song list uses, where it differs from the game's (None = same).
SONGS2 = {
    1: ('Come Rain Come Shine', 'Jenn Cuneta', None), 2: ('Tribulations', 'LCD Soundsystem', None),
    3: ('Tootsee Roll', 'The Block Brothers & Hollywood J', None), 4: ('We Got The Beat', "Pop n' Fresh", None),
    5: ('I Want Candy', "Pop n' Fresh", None), 6: ('Black or White', 'Prince Royal', None),
    7: ('Walking On Sunshine', 'The Flash', None), 8: ("FIVE O'Clock", 'flow', None),
    9: ('Red Alert', 'Basement Jaxx', None), 10: ("Can't Help Falling In Love", 'Cut N Edge', None),
    11: ('My Destiny', 'ASHER', None), 12: ('Nite-Runner', 'FRAZ', None), 13: ('Bust A Move', 'Young MC', None),
    14: ("Don't You (Forget About Me)", 'NuFoundation', None),
    15: ("You're The One That I Want", 'DAVE V & TAYA', None), 16: ('Umbrella', 'Haley Hunt', None),
    17: ('All Good Things (Come to an End)', 'Hamel & Naughty G.', None),
    18: ('I Ran', 'Spacebar vs. Naughty G', None), 19: ('Scramble', 'System 7', None),
    20: ('D.A.N.C.E.', 'Justice', None), 21: ('Feel Together', 'Mickey Disco', None),
    22: ('Makes Me Wonder', 'Sunshine Superman', None), 23: ('Everybody Dance', 'TSMV', None),
    24: ('Obsession', 'Brooklyn Fire', None), 25: ('Call On Me', 'flow', None),
    26: ('JUST BELIEVE', 'Lea Drop feat.Marissa Ship', None),
    27: ('SUPER HERO', 'DJ YOSHITAKA feat.Michaela Thurlow', None),
    28: ('INTO YOUR HEART (Ruffage remix)', 'NAOKI feat.YASMINE', None),
    29: ('LOVING YOU (Epidemik remix)', 'TONI LEO', None), 30: ('I WANT YOUR LOVE (Darwin remix)', 'GAV', None),
    31: ('STAY (Joey Riot remix)', 'DANNY D', None), 32: ('REACH THE SKY (Orbit1 remix)', 'TAYA', None),
    33: ('No Matter What', 'jun feat.Rita Boudreau', None), 34: ('escape', 'U1 & Krystal B', None),
    35: ("Settin' the Scene", 'U1 night style', None), 36: ('Somehow You Found Me', 'DIGI-SEQ-BAND2000', None),
    37: ('Unity', 'The Remembers', None), 38: ('My Love', 'NM feat.Melissa Petty', None),
    39: ('Desert Journey', 'dj TAKA', None), 40: ("Dreamin'", 'TOMOSUKE feat. Adreana', None),
    41: ('The Lonely Streets', 'DJ Yoshitaka feat.Robert "RAab" Stevenson', None),
    42: ('KYOKA-SUIGETSU-ROW (DDR EDITION)', 'TËЯRA feat.Uchusentai NOIZ', None),
    43: ('Habibe (Antuh muhleke)', 'Wendy Parr', None),
    44: ("Racing with Time (NAOKI's 999 remix)", 'jun feat.Godis (Heather Twede)', None),
    45: ('Closer to my Heart (jun remix)', 'NM feat.Heather Elmer', None),
    46: ('We Can Win the Fight', 'D-crew feat.Matt Tucker', None),
    47: ('Open Your Eyes', 'NM feat.JaY_bEe (JB Ah-Fua)', None), 48: ('Lesson2 by DJ', 'MC DDR', None),
    49: ('SILVER☆DREAM', 'jun', None),
    50: ('osaka EVOLVED -MAIDO,OHKINI!- (Ver.A)', 'NAOKI underground', None),
    51: ('osaka EVOLVED -MAIDO,OHKINI!- (Ver.B)', 'NAOKI underground', None),
    52: ('osaka EVOLVED -MAIDO,OHKINI!- (Ver.C)', 'NAOKI underground', None),
    53: ('Lesson by DJ', 'U.T.D & Friends', None), 54: ('Break the Chain', 'Tourbillon', None),
    55: ('CRAZY GONNA CRAZY', 'TRF', None), 56: ('Purple Line', '東方神起', None),
    57: ('Superstar', 'tomboy', None), 58: ('Bigger Than Big (Original Vocal Mix)', 'Super Mal feat. Luciana', None),
    59: ('炎神戦隊ゴーオンジャー', 'EG-PROJECT', 'Engine Sentai Go-onger'),
    60: ('アナタボシ', 'EG-PROJECT', 'Anata Boshi'), 61: ('resonance', 'NAOKI-EX', None),
    62: ('HOUSE NATION', 'ravex', None), 63: ('Our Song', 'Shinichi Osawa', None),
    64: ('NO CRIME', 'SHANADOO', None), 65: ('Brilliant 2U', 'NAOKI', None), 66: ("Keep on movin'", 'NM', None),
    67: ('CELEBRATE NIGHT', 'NAOKI', 'Celebrate Nite'), 68: ('FREE', 'NM PRESENTS', None),
}
# HP2 song -> index k of its banner select_cmn_jp.bin #001/#004.k (banner 49 is osaka EVOLVED's
# folder banner; the three versions' own are 66..68)
BANNER2 = {n: n - 1 for n in range(1, 50)}
BANNER2.update({50: 66, 51: 67, 52: 68, 53: 50})
BANNER2.update({n: n - 3 for n in range(54, 69)})
SONGS3 = {
    1: ('Journey through the Decade', 'GACKT', None), 2: ('迷宮バタフライ', 'ほしな歌唄 (CV: 水樹奈々)', 'Mystery Butterfly'),
    3: ('「ウイリアム・テル」序曲より', 'MC F 40', 'William Tell Overture'),
    4: ('「カルメン」前奏曲', 'ヴァイオレント・ストリング・アンサンブル', 'Carmen Prelude'),
    6: ("CAT'S EYE", '', None), 7: ('素直になれたら', '', 'If You Can Be Honest'),
    8: ('おどるポンポコリン', '', 'DANCING POMPOKOLIN'), 9: ('空色デイズ', '', 'Sorairo days'),
    10: ('創聖のアクエリオン', '', 'Genesis Of Aquarion'), 11: ('ブルーバード', '', 'Blue Bird'),
    12: ('羞恥心', '', 'Shuuchishin'), 13: ('A Perfect Sky', '', None),
    14: ('夢をかなえてドラえもん', '', 'Doraemon, Make My Dreams Come True'),
    15: ("Climax Jump pop'n form", '', None), 16: ('シャングリラ', '', 'Shangri-La'),
    17: ('侍戦隊シンケンジャー', '', 'Samurai Sentai Shinkenger'), 18: ('歩み', '', 'Ayumi'),
    19: ('雫', '', 'Droplets'), 20: ('HOT LIMIT', '', None), 21: ('BUTTERFLY', 'SMiLE.dk', None),
    22: ('君を守って君を愛して', '', 'I Protect You, I Love You'), 23: ('Summer rain', '', None),
    24: ('Stairway Generation', '', None), 25: ('MY BOY', '', None),
    26: ('きみもとべるよ！(ピーターパン)', '', 'You Can Fly! You Can Fly! You Can Fly!'),
    27: ('イッツ・ア・スモールワールド', '', "It's a Small World"),
    28: ('ミッキーマウス・クラブ・マーチ', '', 'Mickey Mouse Club March'),
    31: ('DYNAMITE RAVE', 'NAOKI', None), 32: ('Taking It To The Sky', 'U1 feat. Tammy S. Hansen', None),
    33: ('What Will Come of Me', 'Black Rose Garden', None), 34: ('A Brighter Day', 'NAOKI feat. Aleisha G', None),
    35: ('Shine', 'TOMOSUKE feat. Adreana', None), 36: ('Crazy Control', 'D-crew with VAL TIATIA', None),
    37: ('La libertad', 'Cheryl Horrocks', None), 38: ('Love Again', 'NM feat. Mr. E.', None),
    39: ('La receta', 'Carlos Coco Garcia', None), 40: ('You are a Star', 'NAOKI feat. Anna Kaelin', None),
    41: ('Sacred Oath', 'TËЯRA', None), 42: ('Heatstroke', 'TAG feat. Angie Lee', None),
    43: ('Freeze', 'nc ft. NRG Factory', None), 44: ('Gotta Dance', 'NAOKI feat. Aleisha G', None),
    45: ('THIS NIGHT', 'jun feat. Sonnet', None), 46: ('KIMONO♥PRINCESS', 'jun', None),
    47: ('roppongi EVOLVED ver.A', 'TAG underground', None), 48: ('roppongi EVOLVED ver.B', 'TAG underground', None),
    49: ('roppongi EVOLVED ver.C', 'TAG underground', None),
    53: ('Be With You (Still Miss You)', 'nc ft.Eddie Kay', None),
    54: ('PARADISE', 'Lea Drop feat. McCall Clark', None), 55: ("Now's The Time", 'Brenda', None),
    56: ('Lesson3 by DJ', 'Dr.DDR', None), 57: ('Lesson by DJ', 'U.T.D & Friends', None),
    58: ('Lesson2 by DJ', 'MC DDR', None), 61: ('HOTTEST PARTY', '', None),
    62: ('フルフル♪パーティー', '', 'HOTTEST PARTY 2'),
}
ZAN_GAMES = {
    'hp2': dict(name='Hottest Party 2', songs=SONGS2, brsar='ddr_brsar', sound='MU_DDR_%03d'),
    'hp3': dict(name='Hottest Party 3', songs=SONGS3, brsar='DDRHP3J_SOUND_brsar', sound='MU_DDR_%03dJ'),
}


def detect_game(X):
    if os.path.isfile(os.path.join(X, 'dol', 'songs.csv')):
        return 'hp1'
    for game, g in ZAN_GAMES.items():
        if os.path.isfile(os.path.join(X, 'sound', g['brsar'], 'sounds.csv')):
            return game
    raise SystemExit('%s: not a HOTTEST PARTY 1 / 2 / 3 extraction' % _tilde(X))


def _key(title):
    return re.sub(r'[^0-9a-z]', '', title.lower())


def read_song_list(path):
    """A DDR World song list -> [(block name, {title key: (basename, title)})]."""
    blocks = [('', {})]
    for row in csv.reader(open(path, encoding='utf-8-sig')):
        if not row or not ''.join(row).strip():
            continue
        if row[0].startswith('//'):
            if len(row) > 2 and row[2].strip():
                blocks.append((row[2].strip(), {}))
            continue
        if len(row) > 2 and row[1].strip():
            blocks[-1][1].setdefault(_key(row[2]), (row[1].strip(), row[2].strip()))
    return [b for b in blocks if b[1]]


def find_basename(song_list, game_name, title):
    """(basename, list title, block) of `title`, searching the game's block first."""
    key = _key(title)
    for name, rows in sorted(song_list, key=lambda b: b[0] != game_name):
        if key in rows:
            return rows[key] + (name,)
    return None


def zan_chart(X, game, n):
    if game == 'hp3':
        j = os.path.join(X, 'ssq', 'Jss9', 'MU_DDR_%03dJ.ss9' % n)
        return j if os.path.exists(j) else os.path.join(X, 'ssq', 'ss9', 'MU_DDR_%03d.ss9' % n)
    return os.path.join(X, 'ssq', 'MU_DDR_%03d.ss9' % n)


def zan_banner(X, game, n):
    if game == 'hp3':
        return os.path.join(X, '2Dcommon', 'comAF_JP.bin_unpacked', '#008.%02d.png' % (n - 1))
    return os.path.join(X, 'select', 'select_cmn_jp.bin_unpacked', '#001', '#004.%02d.png' % BANNER2[n])


def gather_zan(X, OUT, game, song_list=None, UNASSIGNED=None):
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import extract_wii_ddr_data as E   # noqa: E402
    g = ZAN_GAMES[game]
    brsar = os.path.join(X, 'sound', g['brsar'])
    sounds = {r['name']: int(r['file']) for r in csv.DictReader(open(os.path.join(brsar, 'sounds.csv')))}
    stems = {}
    for grp in os.listdir(brsar):
        gd = os.path.join(brsar, grp)
        if os.path.isdir(gd):
            for f in os.listdir(gd):
                if f.endswith('.rwsd'):
                    stems[int(f[4:7])] = os.path.join(gd, f[:-5])
    UNASSIGNED = UNASSIGNED or OUT.rstrip(os.sep) + '_unassigned'
    rows = {OUT: [], UNASSIGNED: []}
    for n, (title, artist, list_title) in sorted(g['songs'].items()):
        hit = find_basename(song_list, g['name'], list_title or title) if song_list else None
        if hit:
            dest, stem = OUT, hit[0]
        else:
            dest, stem = (UNASSIGNED if song_list else OUT), safe(title)
        os.makedirs(dest, exist_ok=True)
        chart = zan_chart(X, game, n)
        shutil.copyfile(chart, os.path.join(dest, stem + '.ssq'))
        file_no = sounds[g['sound'] % n]
        waves = E.rwsd_waves(open(stems[file_no] + '.rwsd', 'rb').read(), open(stems[file_no] + '.waves', 'rb').read())
        E.wave_to_wav(os.path.join(dest, stem + '.wav'), waves[0])
        banner = zan_banner(X, game, n)
        shutil.copyfile(banner, os.path.join(dest, stem + '.png'))
        rows[dest].append([n, stem, title, artist, hit[1] if hit else '', hit[2] if hit else '',
                           os.path.relpath(chart, X), '%s (file%03d)' % (g['sound'] % n, file_no),
                           os.path.relpath(banner, X), '%.1f' % (waves[0]['samples'] / waves[0]['rate'])])
        print('%2d %-8s %-50s -> %s' % (n, hit[0] if hit else '-', title, _tilde(dest)))
    for dest, rs in rows.items():
        if rs:
            with open(os.path.join(dest, 'index.csv'), 'w', newline='', encoding='utf-8') as f:
                w = csv.writer(f)
                w.writerow(['song', 'file', 'title', 'artist', 'list_title', 'list_block', 'chart', 'audio', 'banner',
                            'seconds'])
                w.writerows(rs)
            print('%d songs -> %s' % (len(rs), _tilde(dest)))


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0],
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('extracted', nargs='?', default='~/Desktop/DDR Wii ISOs/hottest_party_extracted')
    ap.add_argument('out', nargs='?', default='~/Desktop/DDR Wii ISOs/hottest_party_songs')
    ap.add_argument('--csv', help='DDR World song list naming the songs by basename (HP2 / HP3)')
    ap.add_argument('--unassigned', help='songs the list lacks (default: <out>_unassigned)')
    a = ap.parse_args()
    X, OUT = os.path.expanduser(a.extracted), os.path.expanduser(a.out)
    game = detect_game(X)
    if game == 'hp1':
        gather(X, OUT)
    else:
        song_list = read_song_list(os.path.expanduser(a.csv)) if a.csv else None
        gather_zan(X, OUT, game, song_list, os.path.expanduser(a.unassigned) if a.unassigned else None)


if __name__ == '__main__':
    main()
