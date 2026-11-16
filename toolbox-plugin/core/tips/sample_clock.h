#pragma once

namespace tips {

/// Fires once per interval of accumulated frame time. Toolbox passes ToolboxPlugin::Update() the frame time in
/// SECONDS (GWToolbox::Update: delta_f = milliseconds / 1000.f), whatever the header comment says.
class SampleClock {
public:
    explicit SampleClock(const float every_seconds) : every_(every_seconds) {}

    /// Adds a frame's time; true when the interval has passed (once, however long the frame was).
    bool tick(const float delta_seconds)
    {
        if (!(delta_seconds > 0.f)) return false; // zero, negative or NaN
        elapsed_ += delta_seconds;
        if (elapsed_ < every_) return false;
        elapsed_ = 0.f;
        return true;
    }

private:
    float every_;
    float elapsed_ = 0.f;
};

} // namespace tips
