# Purpose

fdedupe is a command line utility used to find and remove duplicate files accross multiple directories and drives.

It is written in Rust and supports Windows, MacOS and Linux. It uses TUI (text user interface).

# Usage Workflow

fdedupe has two modes:

* Scan is used to find duplicated files. It scans directories recursively and stores information about files in a local database.

* List is used to list duplicates from the database.

* Remove is used to remove duplicates. It will prompt to ask which file to keep from a set of duplicates. It can also use priority rules to remove files without prompting the user. These rules are persisted across sessions in the database.

## Scan

Initiated by specifying the 'scan' operation.

Input parameter is a list of directories. If omitted, the current directory is used.

Options:
- recursive scan (default off)
- rescan - by default when starting to scan a directory the tool checks the database to see if the directory has already been scanned, and will skip it in this case. This is done for every directory encountered during a recursive scan.
- follow symlinks (default off)
- hidden - by default hidden directories are not scanned
- include and exclude - using glob syntax, specify which files to include or exclude

fdedupe will look for an fdedupe_options YAML file in the current directory (priority) and the directory of the fdedupe executable and will read options from that file. Command line options override options file settings.

fdedupe will always resolve directory and file paths to the canonical (physical) path wne using it as the identity of the file or directory. This way if scanning arrives at the same file / directory via different bindings or symlinks, it can still recognize it as the same.

Scanning will scan the specified directories (recursively if needed). 

For each directory:

- Checks the database to see if the directory has already been scanned, and will skip it in this case unless rescan is specified

- Enumerates files and subdirectories both in the file system and (if directory already exists there) in the database

- Identifies deleted files and subdirectories, removes them from the database. Be careful to handle hidden files correctly - if the scan doesn't include hidden files, deletion detection should skip them too.

- Checks each file in the database, if it exists with the same path, file size and last modified timestamp, it assumes the file is unchanged

- If the file is new or changed, computes a fast hash for the file below

- If a file with the same file size and fast hash exists in the database, scan will compute a full hash for all such files (if a full hash doesn't exist yet)

- For each subdirectory, if recursive scan is specified, add it to the list of directories to be scanned. Only follow symlinks if specified by opetion.

### User interface

Scan should show the status of scanning in the console

## List

Initiated by specifying the 'list' operation.

Input parameter is a single directory. If omitted, the current directory is used.

Options:
- recursive list (default off)
- follow symlinks (default off)
- interactive (default off)

For the directory specified in the input, list will print out the following data:

- Canonical (physical) path of the directory
- number and total size of duplicate files under the directory (directly or recursively), based on the database
- list of subdirectories with duplicate files anywhere under them, total number and total size of duplicates
- list of duplicate files directly in the directory, with size

If recursive list is specified list should recursively print the above information for all subdirectories.

Symlinks should only be followed recursively (both for totalling up duplicate files and for recursive listing) if specified.

### Interactive mode

Interactive mode uses recursive totalling of duplicate files, the recursive option is ignored.

It will print the Canonical (physical) path of the directory and number and total size of duplicate files under the directory and then will show the list of subdirectories and files in an interactive console based UI. 

It should print only the number of items that fit in the console window, the user should be able to move a selection and scroll this list with up/down/pgup/pgdown. Pressing right arrow, space or enter when a subdirectory is selected should switch the display of information / list to that directory. Backspace / back arrow should navigate back to the parent. Navigation should stop at the directory specified in the input (should act as root).

