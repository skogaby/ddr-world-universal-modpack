"""Where the ported dancers and stages live.

They are not part of this repository: the sources ported from other DDR releases are
kept in the assets repository (skogaby/ddr-world-universal-modpack-assets), laid out as
`dancers/<Source>/…` and `stages/<Source>/…`. Only the `Custom` sources remain under
this repository's `data_mods/custom_models/`.

No Blender import here, so the lookup can be used and tested with plain Python.
"""

import os

REPO = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..'))
ASSETS_REPO_NAME = 'ddr-world-universal-modpack-assets'


def assets_root():
    """The assets checkout: `$DDR_WORLD_ASSETS`, else the folder of that name beside
    this repository. Returned whether or not it exists, so a script whose paths are all
    overridden still runs without it."""
    override = os.environ.get('DDR_WORLD_ASSETS')
    if override:
        return os.path.abspath(os.path.expanduser(override))
    return os.path.join(os.path.dirname(REPO), ASSETS_REPO_NAME)
