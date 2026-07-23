# Py2App GUI

A simple graphical user interface (GUI) application for packaging Python scripts into standalone executables using PyInstaller and py2app.

## Description

Py2App GUI provides an easy-to-use interface for converting Python scripts into executable applications. It supports cross-platform builds for Windows, macOS, and Linux, automatically selecting the appropriate packaging tool based on the target operating system.

- **macOS**: Uses py2app for creating .app bundles
- **Windows & Linux**: Uses PyInstaller for creating single executable files

## Features

- Simple GUI for selecting script path and output directory
- Cross-platform support (Windows, macOS, Linux)
- Automatic tool selection based on target OS
- Real-time build process feedback with live logs
- Error handling and user notifications
- Browse buttons for easy file/directory selection

## Requirements

- Python 3.x
- PyQt5
- PyInstaller (for Windows/Linux builds)
- py2app (for macOS builds)

## Installation

1. Clone or download this repository
2. Install the required dependencies:

```bash
pip install PyQt5 pyinstaller py2app
```

## Usage

1. Run the application:

```bash
python main.py
```

2. In the GUI:
   - Use the "Browse" button next to "Script Path" to select your Python script (.py file)
   - Use the "Browse" button next to "Output Directory" to select where to save the executable (optional)
   - Select the target operating system from the dropdown
   - Click "Build"

3. Monitor the build progress in the "Build Log" area
4. The application will generate the executable in the specified output directory

## Building the GUI Itself

To package the Py2App GUI into an executable:

1. Select `main.py` as the script
2. Choose macOS as the target OS
3. Click "Build"

This will create a standalone .app bundle for the GUI application that includes setuptools and can be used to package other Python scripts on macOS. Note: The packaged GUI relies on system-installed PyInstaller for building Windows/Linux executables.

## Troubleshooting

- Ensure all dependencies are installed
- Check that the Python script to package has no syntax errors
- For macOS builds, py2app requires setuptools
- If builds fail, check the build log for detailed error messages

## License

This project is open-source. Feel free to modify and distribute.

## Author

Created for simplifying Python application packaging.

## How It Works

The application dynamically generates the necessary setup files and runs the appropriate packaging tool:

- For macOS: Creates a `setup.py` file with py2app configuration and runs `python setup.py py2app`
- For Windows/Linux: Runs PyInstaller with `--onefile` option

## Contributing

Feel free to submit issues and pull requests to improve the application.

## License

This project is open source. Please check for any specific licensing information.
