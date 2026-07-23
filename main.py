import sys
import os
from PyQt5.QtWidgets import QApplication, QWidget, QLabel, QLineEdit, QComboBox, QPushButton, QVBoxLayout, QMessageBox, QFileDialog, QHBoxLayout, QTextEdit
from PyQt5.QtCore import QProcess

class PyInstallerGUI(QWidget):
    def __init__(self):
        super().__init__()
        self.initUI()

    def initUI(self):
        self.setWindowTitle('PyInstaller GUI')

        layout = QVBoxLayout()

        # Script Path
        script_layout = QHBoxLayout()
        self.script_label = QLabel('Script Path:')
        self.script_edit = QLineEdit()
        self.script_browse = QPushButton('Browse')
        self.script_browse.clicked.connect(self.browse_script)
        script_layout.addWidget(self.script_label)
        script_layout.addWidget(self.script_edit)
        script_layout.addWidget(self.script_browse)
        layout.addLayout(script_layout)

        # Output Directory
        output_layout = QHBoxLayout()
        self.output_label = QLabel('Output Directory:')
        self.output_edit = QLineEdit()
        self.output_browse = QPushButton('Browse')
        self.output_browse.clicked.connect(self.browse_output)
        output_layout.addWidget(self.output_label)
        output_layout.addWidget(self.output_edit)
        output_layout.addWidget(self.output_browse)
        layout.addLayout(output_layout)

        self.os_label = QLabel('Target OS:')
        self.os_combo = QComboBox()
        self.os_combo.addItems(['Windows', 'macOS', 'Linux'])
        layout.addWidget(self.os_label)
        layout.addWidget(self.os_combo)

        self.build_button = QPushButton('Build')
        self.build_button.clicked.connect(self.build)
        layout.addWidget(self.build_button)

        self.log_label = QLabel('Build Log:')
        layout.addWidget(self.log_label)
        self.log_edit = QTextEdit()
        self.log_edit.setReadOnly(True)
        layout.addWidget(self.log_edit)

        self.setLayout(layout)

    def browse_script(self):
        file_name, _ = QFileDialog.getOpenFileName(self, 'Select Python Script', '', 'Python Files (*.py)')
        if file_name:
            self.script_edit.setText(file_name)

    def browse_output(self):
        dir_name = QFileDialog.getExistingDirectory(self, 'Select Output Directory')
        if dir_name:
            self.output_edit.setText(dir_name)

    def update_log(self):
        output = self.process.readAll().data().decode()
        self.log_edit.append(output)

    def build(self):
        script = os.path.abspath(self.script_edit.text())
        output = self.output_edit.text()
        os_selected = self.os_combo.currentText()

        if not script:
            QMessageBox.warning(self, 'Error', 'Please enter script path')
            return

        self.log_edit.clear()
        self.log_edit.append(f'Starting build for {os_selected}...\n')

        self.process = QProcess(self)
        self.process.setProcessChannelMode(QProcess.MergedChannels)  # Merge stdout and stderr
        self.process.readyRead.connect(self.update_log)
        self.process.finished.connect(self.on_finished)

        if os_selected == 'macOS':
            # Use py2app
            setup_content = """
from setuptools import setup

APP = ['{script}']
DATA_FILES = []
OPTIONS = {{
    'py2app': {{
        'packages': ['setuptools', 'py2app'],
    }}
}}
setup(
    app=APP,
    data_files=DATA_FILES,
    options=OPTIONS,
)
""".format(script=script)
            with open('setup.py', 'w') as f:
                f.write(setup_content)
            args = ['setup.py', 'py2app']
            if output:
                # py2app uses --dist-dir
                args.extend(['--dist-dir', output])
            self.process.start(sys.executable, args)
        else:
            # Use PyInstaller for Windows and Linux
            args = ['--onefile', script]
            if output:
                args.extend(['--distpath', output])
            self.process.start('/Library/Frameworks/Python.framework/Versions/3.11/bin/pyinstaller', args)

        if not self.process.waitForStarted():
            self.log_edit.append('Failed to start the build process\n')
            QMessageBox.warning(self, 'Error', 'Failed to start the build process')

    def on_finished(self, exit_code, exit_status):
        if exit_code == 0:
            self.log_edit.append('Build completed successfully\n')
            QMessageBox.information(self, 'Success', f'Built for {self.os_combo.currentText()}')
        else:
            output = self.process.readAll().data().decode()
            self.log_edit.append(f'Build failed with exit code {exit_code}: {output}\n')
            QMessageBox.warning(self, 'Error', f'Build failed with exit code {exit_code}: {output}')

if __name__ == '__main__':
    app = QApplication(sys.argv)
    gui = PyInstallerGUI()
    gui.show()
    sys.exit(app.exec_())