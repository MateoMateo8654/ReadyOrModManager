from pathlib import Path
import json

DATA_DIR = Path.home() / "Documents" / "RoM" / "Data"
CONFIG_PATH = DATA_DIR / "config.json"

def load():
    with open(CONFIG_PATH, 'r') as f:
        data = json.load(f)
    return data

def get(key):
    return load()[key]

def set(key,value):
    data = load()
    data[key] = value
    print(data)
    with open(CONFIG_PATH, 'w') as f:
        data = json.dump(data, f, indent=4)
    return data
