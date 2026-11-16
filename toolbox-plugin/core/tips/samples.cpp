#include "tips/samples.h"

namespace tips {

void Latest::add_sample(const float x, const float y, const Tp now, const Ms every)
{
    if (last_sample && std::chrono::duration_cast<Ms>(now - *last_sample) < every) return;
    last_sample = now;
    samples.emplace_back(x, y);
    while (samples.size() > kMaxPoints) samples.pop_front();
}

void Latest::restore_samples(const uint64_t zone, std::vector<std::pair<float, float>> unsent)
{
    if (zone != zone_gen) return;
    unsent.insert(unsent.end(), samples.begin(), samples.end());
    const std::size_t skip = unsent.size() > kMaxPoints ? unsent.size() - kMaxPoints : 0;
    samples.assign(unsent.begin() + static_cast<std::ptrdiff_t>(skip), unsent.end());
}

void Latest::clear_samples()
{
    samples.clear();
    last_sample.reset();
}

} // namespace tips
