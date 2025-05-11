import os
import shutil
import tempfile
import zipfile
import py7zr
import shutil
from pathlib import Path
import tkinter as tk
from tkinter import filedialog
from dependencies import profiles
import json

MODS_FOLDER = Path.home() / "Documents" / "RoM" / "Mods"

def test(test1:str, test2:list[str]):
    pass


def delete_mod(name):
    file = MODS_FOLDER / name
    os.remove(file)


def extract_paks_from_archives():
    """
    extracts all .pak files out of a compressed folder insinde of [...]/RoM/Mods
    """
    for archive in [p for p in MODS_FOLDER.rglob("*") if p.suffix.lower() in ['.zip', '.7z', '.rar']]:
        with tempfile.TemporaryDirectory() as tmpdirname:
            tmpdir = Path(tmpdirname)
            success = False

            try:
                if archive.suffix == '.zip':
                    with zipfile.ZipFile(archive, 'r') as z:
                        z.extractall(tmpdir)
                    success = True
                elif archive.suffix == '.7z':
                    with py7zr.SevenZipFile(archive, mode='r') as z:
                        z.extractall(path=tmpdir)
                    success = True
                else:
                    print(f"Unsupported format: {archive}")
            except Exception as e:
                print(f"Failed to extract {archive}: {e}")

            if success:
                for pak_file in tmpdir.rglob("*.pak"):
                    target_path = MODS_FOLDER / pak_file.name
                    if not target_path.exists():
                        shutil.move(str(pak_file), target_path)
                    else:
                        print(f"Skipped existing: {target_path}")
                for json_file in tmpdir.rglob("*.json"):
                    try:
                        with open(json_file, 'r', encoding='utf-8') as f:
                            data = json.load(f)

                            # Check for the 'mods' key or any other keys you want to check
                            if 'mods' in data and 'name' in data:
                                # You can save or process the profile as needed
                                profiles.save(data)
                                print(f"Processed {json_file} with 'mods' key:\n{json.dumps(data, indent=4)}\n")
                            else:
                                print(f"Skipping {json_file}, no 'mods' key found.")
                    except Exception as e:
                        print(f"Could not read {json_file}: {e}")

                try:
                    archive.unlink()
                    print(f"Deleted archive: {archive}")
                except Exception as e:
                    print(f"Could not delete {archive}: {e}")





def clear_active_mods(path):
    """
    deletes all files in path
    :param path: path to [...]/paks/RoM
    """
    if not os.path.isdir(path):
        print("There was an Error clearing Active Mods")
    else:

        for filename in os.listdir(path):
            file_path = os.path.join(path, filename)
            if os.path.isfile(file_path):
                os.remove(file_path)



def load_mods(destination_path, filenames):
    """
    Synchronizes the mods in the paks/RoM folder by ensuring that all files in the filenames list are present
    and that no extra files exist in the destination path.

    :param destination_path: path to [...]/paks/RoM
    :param filenames: List with filenames of mods to ensure are present in the destination_path
    """
    if not os.path.isdir(destination_path):
        print(f"Error: The directory {destination_path} does not exist.")
        return

    for name in filenames:
        source_file = Path(MODS_FOLDER) / name
        destination_file = Path(destination_path) / name

        if not source_file.exists():
            print(f"Error: Source file {source_file} does not exist.")
        else:
            if not destination_file.exists():
                print(f"File {name} is missing from the destination, copying it now.")
                shutil.copy2(source_file, destination_path)
            else:
                print(f"File {name} is already present in the destination.")

    for filename in os.listdir(destination_path):
        file_path = os.path.join(destination_path, filename)

        if os.path.isfile(file_path) and filename not in filenames:
            print(f"Removing extra file: {filename}")
            os.remove(file_path)

def get_all_mods():
    return [f.name for f in MODS_FOLDER.glob("*.pak") if f.is_file()]

def get_all_active_mods(path):
   return [f.name for f in Path(path).glob("*.pak") if f.is_file()]

def export_profile(name):
    # Load profile data
    data = profiles.load_by_name(name)
    mods = data["mods"]

    # Create a temporary directory to stage the files
    with tempfile.TemporaryDirectory() as tmpdirname:
        tmpdir = Path(tmpdirname)

        # Copy all mod files into the temporary directory
        for mod_name in mods:
            mod_path = MODS_FOLDER / mod_name
            if mod_path.exists():
                shutil.copy2(mod_path, tmpdir / mod_name)
            else:
                print(f"Warning: Mod {mod_name} not found in {MODS_FOLDER}")

        # Save profile.json
        profile_json_path = tmpdir / "profile.json"
        print(data)
        with open(profile_json_path, 'w', encoding='utf-8') as f:
            json.dump(data, f, indent=4)

        # Ask the user where to save the zip file
        root = tk.Tk()
        root.withdraw()  # Hide main window
        save_path = filedialog.asksaveasfilename(
            defaultextension=".zip",
            filetypes=[("ZIP files", "*.zip")],
            title="Save Profile Export As"
        )
        root.destroy()

        if save_path:
            # Create the zip file
            with zipfile.ZipFile(save_path, 'w', zipfile.ZIP_DEFLATED) as zipf:
                for file_path in tmpdir.iterdir():
                    zipf.write(file_path, arcname=file_path.name)

            print(f"Profile exported to {save_path}")
        else:
            print("Export cancelled by user.")


def export_all():
    # Load all profiles
    all_profiles = profiles.load_profiles()

    # Create a temporary directory to stage the files
    with tempfile.TemporaryDirectory() as tmpdirname:
        tmpdir = Path(tmpdirname)

        # Create a directory for all mods and profiles
        mods_dir = tmpdir / "mods"
        mods_dir.mkdir()

        profiles_dir = tmpdir / "profiles"
        profiles_dir.mkdir()

        # Process each profile
        for profile in all_profiles:
            profile_name = profile["name"]

            # Save profile.json in the profiles folder
            profile_json_path = profiles_dir / f"{profile_name}.json"
            with open(profile_json_path, 'w', encoding='utf-8') as f:
                json.dump(profile, f, indent=4)

        # Copy all mods from the MODS_FOLDER into the mods folder
        for mod_file in MODS_FOLDER.glob("*.pak"):
            shutil.copy2(mod_file, mods_dir / mod_file.name)

        # Ask the user where to save the zip file
        root = tk.Tk()
        root.withdraw()  # Hide main window
        save_path = filedialog.asksaveasfilename(
            defaultextension=".zip",
            filetypes=[("ZIP files", "*.zip")],
            title="Save All Profiles Export As"
        )
        root.destroy()

        if save_path:
            # Create the zip file and add all the directories and files
            with zipfile.ZipFile(save_path, 'w', zipfile.ZIP_DEFLATED) as zipf:
                # Add mods folder
                for file_path in mods_dir.rglob("*"):
                    zipf.write(file_path, arcname=file_path.relative_to(tmpdir))

                # Add profiles folder
                for file_path in profiles_dir.rglob("*"):
                    zipf.write(file_path, arcname=file_path.relative_to(tmpdir))

            print(f"All profiles and mods exported to {save_path}")
        else:
            print("Export cancelled by user.")


def uninstall(path):
    # Default RoM folder path
    rom_path = Path.home() / "Documents" / "RoM"

    # List the paths to be deleted (the provided path and the default RoM path)
    target_paths = [rom_path, Path(path)]

    for target_path in target_paths:
        if target_path.exists() and target_path.is_dir():
            try:
                shutil.rmtree(target_path)
                print(f"Successfully uninstalled: {target_path}")
            except Exception as e:
                print(f"Failed to uninstall {target_path}: {e}")
        else:
            print(f"Path does not exist or is not a directory: {target_path}")



