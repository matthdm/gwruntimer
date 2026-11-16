#include "tips/uplink.h"

#include <random>

namespace tips {

namespace {
constexpr std::size_t kQueueCapacity = 64;
constexpr auto kWake = std::chrono::milliseconds(250);

/// 32 hex characters from the OS entropy source. Not a secret, just unique per load.
std::string new_session_id()
{
    std::random_device rd;
    static constexpr char hex[] = "0123456789abcdef";
    std::string out;
    for (int i = 0; i < 16; ++i) {
        const auto b = static_cast<unsigned char>(rd() & 0xFF);
        out += hex[b >> 4];
        out += hex[b & 0xF];
    }
    return out;
}
} // namespace

Uplink::Uplink(std::unique_ptr<Transport> transport, Config initial, const Timing timing, std::string session)
    : transport_(std::move(transport))
    , engine_(
          shared_, *transport_, timing, [] { return std::chrono::steady_clock::now(); },
          [this](const Ms d) {
              // A backoff sleep that a shutdown cuts short.
              std::unique_lock l(queue_m_);
              queue_cv_.wait_for(l, d, [this] { return stop_; });
          },
          session.empty() ? new_session_id() : std::move(session))
{
    shared_.config = std::move(initial);
    shared_.config.token = clean_token(shared_.config.token);
    shared_.sample_every = timing.sample;
    engine_.set_cancel([this] {
        std::lock_guard l(queue_m_);
        return stop_;
    });
    worker_ = std::thread([this] { run(); });
}

Uplink::~Uplink()
{
    signal_stop();
    if (worker_.joinable()) worker_.join();
}

void Uplink::signal_stop()
{
    {
        std::lock_guard l(queue_m_);
        stop_ = true;
    }
    queue_cv_.notify_all();
    transport_->abort(); // after stop_ is set, so the request that fails is not retried
}

void Uplink::send(const Cmd cmd)
{
    {
        std::lock_guard l(queue_m_);
        if (stop_ || queue_.size() >= kQueueCapacity) return; // never block the game: drop it
        queue_.push_back(cmd);
    }
    queue_cv_.notify_one();
}

void Uplink::run()
{
    for (;;) {
        std::optional<Cmd> cmd;
        {
            std::unique_lock l(queue_m_);
            queue_cv_.wait_for(l, kWake, [this] { return stop_ || !queue_.empty(); });
            if (stop_) break;
            if (!queue_.empty()) {
                cmd = queue_.front();
                queue_.pop_front();
            }
        }
        if (cmd) {
            {
                std::lock_guard g(shared_.m);
                shared_.status.busy = true;
            }
            engine_.handle(*cmd);
            {
                std::lock_guard g(shared_.m);
                shared_.status.busy = false;
            }
        }
        engine_.tick();
    }
    engine_.shutdown(); // the game or the tracker is closing: best-effort tidy end of whatever is on
    {
        std::lock_guard g(shared_.m);
        shared_.status.busy = false;
    }
    stopped_ = true;
}

Config Uplink::config() const
{
    std::lock_guard g(shared_.m);
    return shared_.config;
}

Status Uplink::status() const
{
    std::lock_guard g(shared_.m);
    return shared_.status;
}

Latest Uplink::latest() const
{
    std::lock_guard g(shared_.m);
    return shared_.latest;
}

bool Uplink::sharing_locked() const
{
    return shared_.status.active_general || shared_.config.share_position;
}

bool Uplink::sharing() const
{
    std::lock_guard g(shared_.m);
    return sharing_locked();
}

void Uplink::update_config(const std::function<void(Config&)>& change)
{
    std::lock_guard g(shared_.m);
    const bool tracking = shared_.status.active_run.has_value() || shared_.status.active_general;
    const auto mode = shared_.config.tracking_mode;
    change(shared_.config);
    if (tracking) shared_.config.tracking_mode = mode; // the mode cannot change while tracking
    shared_.config.token = clean_token(shared_.config.token);
}

void Uplink::push_zone(const uint32_t map_id, const bool explorable, const std::optional<Bounds> bounds)
{
    {
        std::lock_guard g(shared_.m);
        auto& l = shared_.latest;
        l.map_id = map_id;
        l.explorable = explorable;
        l.bounds = bounds;
        l.pos.reset();        // the old position belongs to the old map
        l.clear_samples();    // and so does its trail
        ++l.zone_gen;
    }
    send(Cmd::Zone);
}

bool Uplink::knows_map() const
{
    std::lock_guard g(shared_.m);
    return shared_.latest.map_id.has_value();
}

bool Uplink::wants_bounds() const
{
    std::lock_guard g(shared_.m);
    return shared_.latest.map_id && !shared_.latest.bounds && sharing_locked();
}

void Uplink::set_bounds(const std::optional<Bounds> bounds)
{
    std::lock_guard g(shared_.m);
    if (sharing_locked()) shared_.latest.bounds = bounds;
}

void Uplink::set_position(std::optional<Pos> pos)
{
    if (pos && !pos->is_sane()) pos.reset();
    const auto now = std::chrono::steady_clock::now();
    std::lock_guard g(shared_.m);
    const bool share = sharing_locked();
    auto& l = shared_.latest;
    l.pos = pos ? std::optional(std::make_pair(*pos, now)) : std::nullopt;
    if (pos && share) l.add_sample(pos->x, pos->y, now, shared_.sample_every);
    else if (!share) l.clear_samples();
}

void Uplink::refresh() { send(Cmd::Refresh); }
void Uplink::start_run() { send(Cmd::Start); }
void Uplink::end_run() { send(Cmd::End); }
void Uplink::start_general() { send(Cmd::StartGeneral); }
void Uplink::end_general() { send(Cmd::EndGeneral); }

} // namespace tips
