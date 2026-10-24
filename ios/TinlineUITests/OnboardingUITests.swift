import XCTest

/// Walks onboarding on a fresh install the way a person would and attaches a screenshot of every screen
/// (`ios/scripts/run_ui.sh` exports them). Buttons are found by their visible text, so a copy change on these
/// screens is a change here too.
final class OnboardingUITests: XCTestCase {
    private var app: XCUIApplication!
    private var shots = 0

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launch()
    }

    private func shot(_ name: String) {
        shots += 1
        let a = XCTAttachment(screenshot: app.screenshot())
        a.name = String(format: "%02d-%@", shots, name)
        a.lifetime = .keepAlways
        add(a)
    }

    private func text(_ s: String) -> XCUIElement { app.staticTexts[s] }

    private func starting(_ prefix: String) -> XCUIElement {
        app.staticTexts.matching(NSPredicate(format: "label BEGINSWITH %@", prefix)).firstMatch
    }

    private func tap(_ label: String) {
        let b = app.buttons[label]
        XCTAssertTrue(b.waitForExistence(timeout: 15), "no button \"\(label)\"")
        b.tap()
    }

    /// The new-account path: nickname, no passphrase, recovery words, device name, terms, permissions, then
    /// "Your line is open". Fails (with the screen attached) if the account could not be saved.
    func testCreateAccount() {
        XCTAssertTrue(text("A direct line to your people").waitForExistence(timeout: 30))
        shot("welcome")
        tap("Create my account")

        XCTAssertTrue(text("What should people call you?").waitForExistence(timeout: 15))
        let field = app.textFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 5))
        field.tap()
        field.typeText("Ada")
        shot("nickname")
        tap("Continue")

        XCTAssertTrue(text("Add a passphrase").waitForExistence(timeout: 15))
        shot("passphrase")
        tap("Skip for now")

        // The remaining screens differ by platform state (save failures, permissions already granted), so advance with
        // whichever forward button the current screen has until "Your line is open".
        let forward = ["I wrote them down", "I saved it elsewhere", "I agree", "Continue", "Not now"]
        let done = starting("Your line is open")
        let failed = text("Couldn\u{2019}t save your account")
        for i in 0..<12 {
            sleep(1)
            if done.exists { break }
            let title = app.staticTexts.firstMatch.label
            shot("step\(i)-" + title.prefix(30).replacingOccurrences(of: " ", with: "_"))
            if failed.exists { XCTFail("the account key could not be saved"); return }
            guard let next = forward.first(where: { app.buttons[$0].exists && app.buttons[$0].isEnabled }) else {
                XCTFail("no forward button on \"\(title)\""); return
            }
            app.buttons[next].tap()
        }
        XCTAssertTrue(done.waitForExistence(timeout: 15), "did not reach \"Your line is open\"")
        shot("done")
    }
}
