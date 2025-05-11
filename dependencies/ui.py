import sys
import os
import subprocess
import shutil
from pathlib import Path
from tkinter import filedialog
from PyQt5.QtWidgets import (
    QApplication, QWidget, QVBoxLayout, QHBoxLayout,
    QLabel, QListWidget, QPushButton, QComboBox,
    QLineEdit, QDialog, QDialogButtonBox, QCheckBox,
    QInputDialog, QMessageBox, QTabWidget
)
from PyQt5.QtCore import QSize, Qt
from PyQt5.QtGui import QDragEnterEvent, QDropEvent
from PyQt5.QtWidgets import QTabWidget
from dependencies import filemanagment
from dependencies import profiles
from dependencies import session
from dependencies import config


class ModManagerApp(QWidget):
    def __init__(self):
        super().__init__()

        self.ROM_ROOT = Path.home() / "Documents" / "RoM"
        self.MODS_DIR = self.ROM_ROOT / "Mods"
        self.RON_PAK_PATH = config.get("RoN_Path")
        self.mods = filemanagment.get_all_mods()
        self.selected_mods = filemanagment.get_all_active_mods(self.RON_PAK_PATH)

        self.available_mods = []
        for mod in self.mods:
            if mod not in self.selected_mods:
                self.available_mods.append(mod)

        self.setWindowTitle("Ready or Mod Manager")
        self.setGeometry(100, 100, 800, 500)

        # Enable drag and drop
        self.setAcceptDrops(True)

        # Initial mod sets
        self.profiles = []
        for profile in profiles.load_profiles():
            self.profiles.append(profile["name"])

        self.selected_mods_set = set(self.selected_mods)
        self.initUI()

    def initUI(self):
        self.tabs = QTabWidget()
        self.tabs.setStyleSheet(dark_stylesheet)

        # Create main tab layout
        main_tab = QWidget()
        main_layout = QVBoxLayout()
        # ... (rest of your existing code that builds main_layout)
        main_tab.setLayout(main_layout)

        # Add the main tab to the QTabWidget
        self.tabs.addTab(main_tab, "Mod Manager")



        # Set QTabWidget as main layout
        container_layout = QVBoxLayout()
        container_layout.addWidget(self.tabs)
        self.setLayout(container_layout)

        # Search bar (placed above the lists)
        self.search_bar = QLineEdit()
        self.search_bar.setMinimumHeight(30)
        self.search_bar.setPlaceholderText("Search mods...")
        self.search_bar.setClearButtonEnabled(True)  # Enable the clear button ("X")
        self.search_bar.textChanged.connect(self.search_mods)  # Keep filtering when text changes
        main_layout.addWidget(QLabel("Search Mods:"))
        main_layout.addWidget(self.search_bar)

        # Mod list layout
        list_layout = QHBoxLayout()

        # Left list (unselected)
        left_layout = QVBoxLayout()
        left_layout.addWidget(QLabel("Available Mods"))
        self.unselected_list = QListWidget()
        self.unselected_list.addItems(self.available_mods)
        self.unselected_list.itemDoubleClicked.connect(self.select_mod)
        left_layout.addWidget(self.unselected_list)

        # Delete Mod button (under the left list)
        delete_mod_btn = QPushButton("Delete Mod")
        delete_mod_btn.setMinimumHeight(20)
        delete_mod_btn.clicked.connect(self.delete_mod)
        left_layout.addWidget(delete_mod_btn)

        list_layout.addLayout(left_layout)

        # Right list (selected)
        right_layout = QVBoxLayout()
        right_layout.addWidget(QLabel("Selected Mods"))
        self.selected_list = QListWidget()
        self.selected_list.addItems(self.selected_mods)
        self.selected_list.itemDoubleClicked.connect(self.deselect_mod)
        right_layout.addWidget(self.selected_list)

        # Reload Mods button (under the right list)
        reload_btn = QPushButton("Reload Mods")
        reload_btn.setMinimumHeight(20)
        reload_btn.clicked.connect(self.reload_mods)
        right_layout.addWidget(reload_btn)

        list_layout.addLayout(right_layout)
        main_layout.addLayout(list_layout)

        # Profile selection
        self.profile_dropdown = QComboBox()
        self.profile_dropdown.addItem("Vanilla")
        self.profile_dropdown.addItems(self.profiles)
        self.profile_dropdown.currentTextChanged.connect(self.profile_changed)

        # Increase the dropdown height and make the list more clickable
        self.profile_dropdown.setMinimumHeight(40)  # Make the combobox itself taller
        self.profile_dropdown.setStyleSheet("font-size: 16px;")  # Optional: Increase font size

        main_layout.addWidget(QLabel("Select Profile:"))
        main_layout.addWidget(self.profile_dropdown)

        # Profile buttons
        profile_btns = QHBoxLayout()
        create_btn = QPushButton("Create Profile")
        create_btn.setMinimumHeight(30)
        create_btn.clicked.connect(self.create_profile)
        save_btn = QPushButton("Save Profile")
        save_btn.setMinimumHeight(30)
        save_btn.clicked.connect(self.save_profile)
        delete_btn = QPushButton("Delete Profile")
        delete_btn.setMinimumHeight(30)
        delete_btn.clicked.connect(self.delete_profile)
        profile_btns.addWidget(create_btn)
        profile_btns.addWidget(save_btn)
        profile_btns.addWidget(delete_btn)
        main_layout.addLayout(profile_btns)

        # Add Export button
        export_btn = QPushButton("Export Profile")
        export_btn.setMinimumHeight(30)
        export_btn.clicked.connect(self.export_profile)

        profile_btns.addWidget(create_btn)
        profile_btns.addWidget(save_btn)
        profile_btns.addWidget(delete_btn)
        profile_btns.addWidget(export_btn)  # Add Export button here

        main_layout.addLayout(profile_btns)

        # Start game
        start_btn = QPushButton("Start Game")
        start_btn.setMinimumHeight(50)
        start_btn.setStyleSheet("font-size: 16px;")
        start_btn.clicked.connect(self.start_game)
        main_layout.addWidget(start_btn)

        self.setLayout(main_layout)

        # Create a second tab
        second_tab = QWidget()
        second_layout = QVBoxLayout()

        # Set the layout's alignment to top
        second_layout.setAlignment(Qt.AlignTop)

        #export_all_profiles
        export_all_btn = QPushButton("Export all mod and profile data")
        export_all_btn.setMinimumHeight(25)
        export_all_btn.clicked.connect(self.export_all_profiles)
        # Creating the buttons
        reset_button = QPushButton("Change ReadyOrNot Path")
        reset_button.setMinimumHeight(25)
        reset_button.clicked.connect(self.reset_ready_or_not_path)

        uninstall_button = QPushButton("Uninstall")
        uninstall_button.clicked.connect(self.uninstall)

        # Adding the buttons to the layout
        second_layout.addWidget(export_all_btn)
        second_layout.addWidget(reset_button)
        second_layout.addWidget(uninstall_button)

        # Setting the layout for the second tab
        second_tab.setLayout(second_layout)

        # Adding the second tab to the QTabWidget
        self.tabs.addTab(second_tab, "Settings")


    def dragEnterEvent(self, event: QDragEnterEvent):
        if event.mimeData().hasUrls():
            event.acceptProposedAction()

    def dropEvent(self, event: QDropEvent):
        for url in event.mimeData().urls():
            file_path = url.toLocalFile()
            if os.path.isfile(file_path):
                try:
                    shutil.copy(file_path, self.MODS_DIR)
                    print(f"Dropped and copied: {file_path} to {destination}")
                except Exception as e:
                    print(f"Error copying file: {e}")
        self.reload_mods()
        self.reload_profiles()

    def select_mod(self, item):
        mod = item.text()
        current_search_term = self.search_bar.text()  # Save current search term

        self.unselected_list.takeItem(self.unselected_list.row(item))
        self.selected_list.addItem(mod)
        self.selected_mods_set.add(mod)
        print(f"Selected: {mod}")

        filemanagment.load_mods(self.RON_PAK_PATH, self.get_current_selected_mods())
        print("Current selected mods:", ', '.join(self.get_current_selected_mods()))
        self.reload_mods()

        # Reapply the search term
        self.search_bar.setText(current_search_term)
        self.search_mods()  # Reapply the search filtering

    def deselect_mod(self, item):
        mod = item.text()
        current_search_term = self.search_bar.text()  # Save current search term

        self.selected_list.takeItem(self.selected_list.row(item))
        self.selected_mods_set.discard(mod)

        if mod in self.available_mods:
            self.unselected_list.addItem(mod)
        print(f"Deselected: {mod}")

        filemanagment.load_mods(self.RON_PAK_PATH, self.get_current_selected_mods())
        print("Current selected mods:", ', '.join(self.get_current_selected_mods()))
        self.reload_mods()

        # Reapply the search term
        self.search_bar.setText(current_search_term)
        self.search_mods()  # Reapply the search filtering

    def delete_mod(self):
        selected_item = self.unselected_list.currentItem()
        if selected_item:
            mod_name = selected_item.text()

            #
            reply = QMessageBox.question(
                self,
                "Delete Mod",
                f"Are you sure you want to delete {mod_name} ?",
                QMessageBox.Yes | QMessageBox.No,
                QMessageBox.No
            )
            if reply == QMessageBox.Yes:
                filemanagment.delete_mod(mod_name)
                self.reload_mods()
                print(f"Deleted mod: {mod_name}")

    def search_mods(self):
        search_term = self.search_bar.text().lower()

        # Filter unselected_list
        for i in range(self.unselected_list.count()):
            item = self.unselected_list.item(i)
            item.setHidden(search_term not in item.text().lower())

        # Filter selected_list
        for i in range(self.selected_list.count()):
            item = self.selected_list.item(i)
            item.setHidden(search_term not in item.text().lower())

    def profile_changed(self, profile):
        print(f"Profile changed to: {profile}")
        mods = self.get_mods_for_profile(profile)

        self.reload_mods()

    def create_profile(self):
        name, ok = QInputDialog.getText(self, "Create Profile", "Enter profile name:")
        if ok and name:
            profiles.save({"name": name, "mods": self.get_current_selected_mods()})
            print(f"Created Profile: {name} with mods: {', '.join(self.get_current_selected_mods())}")
            self.reload_profiles()

    def save_profile(self):
        profile = self.profile_dropdown.currentText()

        reply = QMessageBox.question(
            self,
            "Save Profile",
            f"Are you sure you want to save the profile {profile}?",
            QMessageBox.Yes | QMessageBox.No,
            QMessageBox.No
        )

        if reply == QMessageBox.Yes:
            profiles.save({"name": profile, "mods": self.get_current_selected_mods()})
            print(f"Saved Profile: {profile} with mods: {', '.join(self.get_current_selected_mods())}")

    def delete_profile(self):
        profile = self.profile_dropdown.currentText()

        reply = QMessageBox.question(
            self,
            "Delete Profile",
            f"Are you sure you want to delete the profile {profile}?",
            QMessageBox.Yes | QMessageBox.No,
            QMessageBox.No
        )

        if reply == QMessageBox.Yes:
            profiles.delete(profile)
            print(f"Deleted Profile: {profile}")
            self.reload_profiles()

    def export_profile(self):
        print('export profile: ', self.profile_dropdown.currentText())
        filemanagment.export_profile(self.profile_dropdown.currentText())

    def export_all_profiles(self):
        filemanagment.export_all()

    def reload_profiles(self):
        print("Reloading Profiles List...")

        self.profiles = [profile["name"] for profile in profiles.load_profiles()]

        self.profile_dropdown.clear()
        self.profile_dropdown.addItem("Vanilla")
        self.profile_dropdown.addItems(self.profiles)
        print("Profiles reloaded.")

    def reload_mods(self):
        print("Reloading Mods List...")

        filemanagment.extract_paks_from_archives()

        self.mods = filemanagment.get_all_mods()
        self.selected_mods = filemanagment.get_all_active_mods(self.RON_PAK_PATH)

        print(f"Selected mods: {self.selected_mods}")

        self.available_mods = [mod for mod in self.mods if mod not in self.selected_mods]

        self.selected_mods_set = {mod for mod in self.selected_mods if mod in self.mods}

        self.unselected_list.clear()
        self.selected_list.clear()

        self.unselected_list.addItems(self.available_mods)

        for mod in self.selected_mods_set:
            self.selected_list.addItem(mod)

        print("Mods reloaded.")

    def start_game(self):
        base_path = os.path.abspath(
            os.path.join(self.RON_PAK_PATH, "..", "..", "..", "..")
        )
        exe_path = os.path.join(base_path, "ReadyOrNot.exe")
        try:
            subprocess.run([exe_path], check=True)
        except subprocess.CalledProcessError as e:
            print(f"Error while running ReadyOrNot: {e}")
        except FileNotFoundError:
            print(f"File Not Found: {exe_path}")

    def get_current_selected_mods(self):
        return [self.selected_list.item(i).text() for i in range(self.selected_list.count())]

    def get_mods_for_profile(self, profile):
        profile_mods = profiles.load_by_name(profile)["mods"]
        filemanagment.load_mods(self.RON_PAK_PATH, profile_mods)
        return profile_mods

    def reset_ready_or_not_path(self):
        valid_path = False
        while not valid_path:
            msg = QMessageBox()
            # msg.setIcon(QMessageBox.Information)
            msg.setWindowTitle("RoM Installer")
            msg.setText(
                "Please Select the Paks folder of your ReadyOrNot Installation\r\n for most users this will be: C:\Program Files (x86)\Steam\steamapps\common\Ready Or Not\ReadyOrNot\Content\Paks")
            msg.setStandardButtons(QMessageBox.Ok)
            msg.setWindowFlags(msg.windowFlags() | Qt.WindowStaysOnTopHint)
            msg.activateWindow()
            msg.raise_()
            msg.exec_()

            RoN_Path = Path(
                filedialog.askdirectory(title="Please Selcet the Pak Folder Of Your Ready or Not installation")) / "RoM"
            if str(Path("ReadyOrNot/Content/Paks")) in str(RoN_Path):
                valid_path = True
            config.set("RoN_Path",str(RoN_Path))

    def uninstall(self):
        reply = QMessageBox.question(
            self,
            "Uninstall RoM",
            f"Are you sure you want to unistall RoM?\r\nThis will remove all RoM Files other then the .exe\r\nIt will result in loss of all profiles and mods",
            QMessageBox.Yes | QMessageBox.No,
            QMessageBox.No
        )

        if reply == QMessageBox.Yes:
            print(f"Uninstalling")
            filemanagment.uninstall(self.RON_PAK_PATH)


dark_stylesheet = """
QWidget {
    background-color: #2b2b2b;
    color: #dcdcdc;
}

QLineEdit, QListWidget, QComboBox, QPushButton {
    background-color: #3c3f41;
    color: #dcdcdc;
    border: 1px solid #5c5c5c;
}

QPushButton:hover {
    background-color: #4c5052;
}

QLabel {
    color: #dcdcdc;
}

QScrollBar:vertical {
    background: #2b2b2b;
    width: 12px;
    margin: 15px 3px 15px 3px;
}

QScrollBar::handle:vertical {
    background: #5c5c5c;
    min-height: 20px;
    border-radius: 5px;
}

QTabWidget {
    background-color: #2b2b2b;
    border: none;  /* Remove the frame around the tabs */
    padding: 0px;  /* Remove internal padding to avoid extra borders */
}

QTabBar {
    border: none;  /* Remove any border around the tab bar itself */
}

QTabBar::tab {
    background-color: #3c3f41;
    color: #dcdcdc;
    border: 1px solid #5c5c5c;
    padding: 10px;
    min-width: 100px;
    margin: 0px;
}

QTabBar::tab:selected {
    background-color: #4c5052;
    border-color: #5c5c5c;  /* Ensure selected tab has matching border */
}

QTabBar::tab:hover {
    background-color: #4c5052;
}

QTabWidget::pane {
    border: none;  /* Remove the white frame around the tabs' contents */
}
"""



def start():
    app = QApplication(sys.argv)
    app.setStyleSheet(dark_stylesheet)
    window = ModManagerApp()
    window.show()
    sys.exit(app.exec_())
