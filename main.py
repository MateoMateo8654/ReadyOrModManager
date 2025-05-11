def main():
    from dependencies import setup
    from dependencies import ui

    setup.setup()
    ui.start()

if __name__ == "__main__":
    main()