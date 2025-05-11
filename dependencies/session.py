from pathlib import Path
import json

session_PATH = Path.home() / "Documents" / "RoM" / "Data" / "session.json"

def load():
    with open(session_PATH, 'r') as f:
        data = json.load(f)
    return data

def get_status():
    return load()["active"]

def get_active_mods():
    return load()["mods"]

def get_active_profile():
    return load()["profile"]

def set(key,data):
    data = load()
    data[key] = data
    with open(session_PATH, 'w') as f:
        data = json.dump(f)
    return data