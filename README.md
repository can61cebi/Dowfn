# Dowfn - Modern Download Manager

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Dowfn is a high-performance, multi-threaded download manager developed in Rust. It features a modern interface and browser integration.

## Screenshots

| Downloading | Completed Downloads | File Explorer |
| :---: | :---: | :---: |
| ![Downloading](https://raw.githubusercontent.com/can61cebi/Dowfn/refs/heads/main/images/1.png) | ![Download completed](https://raw.githubusercontent.com/can61cebi/Dowfn/refs/heads/main/images/2.png) | ![View in file explorer](https://raw.githubusercontent.com/can61cebi/Dowfn/refs/heads/main/images/3.png) |
*Real-time download status with speed and progress indicators.*

## ✨ Features

- **Multi-threaded Downloading:** Splits files into chunks and downloads them using multiple threads to maximize download speed.
- **Modern UI:** An intuitive and easy-to-use desktop application built with `Slint`.
- **Pause and Resume:** Pause your downloads at any time and resume them later from where you left off.
- **Browser Integration:** Directly capture downloads from your browser and manage them through the application with the browser extension.
- **Real-time Statistics:** Track instant download speed, amount of data downloaded, and remaining time.
- **Purely Rust:** Leverages the power of Rust for performance, safety, and concurrency.

## 🏗️ Project Architecture

The project is developed modularly within a Rust workspace:

- **`dowfn-core`**: The heart of the application. It contains all the core logic, such as download management, chunk handling, pause/resume functionality, and file merging. It runs fully asynchronously with `Tokio`.
- **`dowfn-app`**: The desktop GUI developed with the `Slint` framework. It communicates with `dowfn-core` via `mpsc` channels.
- **`dowfn-shared`**: Contains common data structures (models, protocols) used across different components of the project (core, app, extension).
- **`dowfn-extension`**: The browser extension developed for browsers. It captures download links and sends them to the main application.
- **`dowfn-native-host`**: The native messaging host that enables communication between the browser extension and the desktop application.

## 🛠️ Tech Stack

- **Core Language:** [Rust](https://www.rust-lang.org/)
- **Asynchronous Runtime:** [Tokio](https://tokio.rs/)
- **GUI Framework:** [Slint](https://slint.rs/)
- **Browser Extension:** JavaScript, HTML, CSS

## 🚀 Installation and Usage

Follow the steps below to compile and run the project on your local machine.

### Prerequisites

- A working [Rust](https://www.rust-lang.org/tools/install) installation.

### Steps

1.  **Clone the repository:**
    ```sh
    git clone https://github.com/can61cebi/Dowfn.git
    cd Dowfn
    ```

2.  **Build the project (Release mode is recommended):**
    ```sh
    cargo build --release
    ```

3.  **Run the application:**
    ```sh
    cargo run -p dowfn-app
    ```

## 🤝 Contributing

Contributions help make the project better! To report a bug or suggest a new feature, please open an issue. Pull requests are always welcome.

1.  Fork the Project.
2.  Create your Feature Branch (`git checkout -b feature/AmazingFeature`).
3.  Commit your Changes (`git commit -m 'Add some AmazingFeature'`).
4.  Push to the Branch (`git push origin feature/AmazingFeature`).
5.  Open a Pull Request.

## 📝 License

This project is licensed under the MIT License. See the `LICENSE` file for more information.

## 📬 Contact

Can Çebi - [@can61cebi](https://github.com/can61cebi) - can@cebi.tr

Project Link: [https://github.com/can61cebi/Dowfn](https://github.com/can61cebi/Dowfn)