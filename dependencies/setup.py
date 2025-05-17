import os
import json
from pathlib import Path
from tkinter import filedialog
from PyQt5.QtWidgets import QApplication, QMessageBox
from PyQt5.QtCore import Qt
import sys
from dependencies import config

# Define directories
ROM_ROOT = Path.home() / "Documents" / "RoM"
DATA_DIR = ROM_ROOT / "Data"
MODS_DIR = ROM_ROOT / "Mods"

app = QApplication(sys.argv)

# Ensure directories exist
def setup():
    for directory in [DATA_DIR, MODS_DIR]:
        os.makedirs(directory, exist_ok=True)
        print(f"Ensured directory exists: {directory}")
    check_if_right_path_exists()
    create_empty_json_files()


def check_if_right_path_exists():
    try:
         if str(Path("ReadyOrNot/Content/Paks")) in str(Path(config.get("RoN_Path"))):
             return True
         else:
             Path.unlink(DATA_DIR / 'config.json' )
             return False
    except:
        if not (DATA_DIR / 'config.json').exists():
            return False
        else:
            Path.unlink(DATA_DIR / 'config.json')
            return False


from PyQt5.QtWidgets import QMessageBox, QFileDialog
from PyQt5.QtCore import Qt
import os, json
from pathlib import Path


def create_empty_json_files():
    for filename in ['profiles.json', 'config.json']:
        file_path = DATA_DIR / filename
        if not file_path.exists():
            with open(file_path, 'w') as f:
                if filename == 'config.json':
                    valid_path = False
                    while not valid_path:
                        msg = QMessageBox()
                        msg.setWindowTitle("RoM Installer")
                        msg.setText(
                            "Please Select the Paks folder of your ReadyOrNot Installation\nfor most users this will be:\nC:\\Program Files (x86)\\Steam\\steamapps\\common\\Ready Or Not\\ReadyOrNot\\Content\\Paks")
                        msg.setStandardButtons(QMessageBox.Ok | QMessageBox.Cancel)
                        msg.setWindowFlags(msg.windowFlags() | Qt.WindowStaysOnTopHint)
                        msg.activateWindow()
                        msg.raise_()

                        result = msg.exec_()
                        if result == QMessageBox.Cancel:
                            exit("User cancelled the folder selection")
                            return

                        selected_path = filedialog.askdirectory(
                            title="Please Select the Pak Folder Of Your Ready or Not installation")
                        if not selected_path:
                            exit("User cancelled the folder selection")
                            return

                        RoN_Path = Path(selected_path) / "RoM"
                        if "ReadyOrNot/Content/Paks" in str(RoN_Path):
                            valid_path = True

                    os.makedirs(RoN_Path, exist_ok=True)
                    print(f"Ensured directory exists: {RoN_Path}")
                    json.dump({"RoN_Path": str(RoN_Path)}, f, indent=4)
                elif filename == 'session.json':
                    json.dump({"active": True, "profile": -1, "mods": []}, f, indent=4)
                else:
                    json.dump([], f, indent=4)
            print(f"Created JSON file: {file_path}")
