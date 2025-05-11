from pathlib import Path
import json
"""
Profile example:

{
"name":"Test1",
"mods": [
        "1.pak",
        "2.pak"
    ]
}

"""



PROFILES_PATH = Path.home() / "Documents" / "RoM" / "Data" / "profiles.json"


def load_profiles():
    with open(PROFILES_PATH,"r") as f:
        data = json.load(f)
    return data

def load_by_name(name):
    for profile in load_profiles():
        if profile["name"] == name:
            return profile
    return {"name":"None","mods":[]}

def save(profile):
    data = load_profiles()
    # Check if the profile with the same name already exists
    for i, existing_profile in enumerate(data):
        if existing_profile["name"] == profile["name"]:
            # Replace the existing profile
            data[i] = profile
            break
    else:
        # If profile does not exist, append the new profile
        data.append(profile)

    # Write the updated data back to the file
    with open(PROFILES_PATH, "w") as f:
        json.dump(data, f, indent=4)


def delete(name):
    """
    Deletes a profile by name from the profiles.json file.
    :param name: The name of the profile to be deleted
    :return: None
    """
    data = load_profiles()

    # Find the profile and remove it
    data = [profile for profile in data if profile["name"] != name]

    # Write the updated data back to the file
    with open(PROFILES_PATH, "w") as f:
        json.dump(data, f, indent=4)



