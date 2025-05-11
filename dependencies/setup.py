import os
import json
from pathlib import Path
from tkinter import filedialog
from PyQt5.QtWidgets import QApplication, QMessageBox
from PyQt5.QtCore import Qt
import sys
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
    create_empty_json_files()


def create_empty_json_files():
    for filename in ['profiles.json', 'config.json']:
        file_path = DATA_DIR / filename
        if not file_path.exists():
            with open(file_path, 'w') as f:
                if filename == 'config.json':
                    valid_path = False
                    while not valid_path:
                        msg = QMessageBox()
                        #msg.setIcon(QMessageBox.Information)
                        msg.setWindowTitle("RoM Installer")
                        msg.setText("Please Select the Paks folder of your ReadyOrNot Installation\r\n for most users this will be: C:\Program Files (x86)\Steam\steamapps\common\Ready Or Not\ReadyOrNot\Content\Paks")
                        msg.setStandardButtons(QMessageBox.Ok)
                        msg.setWindowFlags(msg.windowFlags() | Qt.WindowStaysOnTopHint)
                        msg.activateWindow()
                        msg.raise_()
                        msg.exec_()

                        RoN_Path = Path(filedialog.askdirectory(title="Please Selcet the Pak Folder Of Your Ready or Not installation")) / "RoM"
                        if str(Path("ReadyOrNot/Content/Paks")) in str(RoN_Path):
                            valid_path = True
                    os.makedirs(RoN_Path, exist_ok=True)
                    print(f"Ensured directory exists: {RoN_Path}")
                    json.dump({"RoN_Path": str(RoN_Path)}, f, indent=4)
                elif filename == 'session.json':
                    json.dump({"active": True,"profile":-1,"mods":[]},f,indent=4)
                else:
                    json.dump([], f, indent=4)
            print(f"Created JSON file: {file_path}")


