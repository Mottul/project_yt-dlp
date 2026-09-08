#!/bin/sh
# Doppelklick auf diese Datei öffnet das Terminal und startet das Programm
# daneben. Kein Tippen nötig.
cd "$(dirname "$0")" || exit 1
exec ./mottulvideoloader
